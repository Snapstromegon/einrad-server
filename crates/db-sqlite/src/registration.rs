//! The rider registration rules on a connection: a rider's single and pair
//! starts (ADR 0004). The repo methods run them in one write transaction,
//! together with their access checks.

use db_core::{
    act_type::ActType,
    clubs::{Actor, RegistrationError},
    gender::Gender,
    ids::{ActId, ClubId, RiderId, UserId},
    people,
    riders::{Pair, Starts},
};
use sqlx::SqliteConnection;

/// Whose riders a pair can be linked with: clubs only link their own riders,
/// the organizer also riders of different clubs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartnerClubs {
    Same,
    Any,
}

/// Fails with `NotFound` for an unknown club and `Forbidden` unless the actor
/// may manage it.
pub(crate) async fn club_access(
    conn: &mut SqliteConnection,
    actor: &Actor,
    club_id: ClubId,
) -> Result<(), RegistrationError> {
    let owner_id = sqlx::query_scalar!(
        r#"SELECT owner_id as "owner_id!: UserId" FROM clubs WHERE id = $1"#,
        club_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    if actor.may_manage_club(club_id, owner_id) {
        Ok(())
    } else {
        Err(RegistrationError::Forbidden)
    }
}

/// Loads the club of a rider and checks that the actor may manage it (see
/// [`club_access`]). Returns the club's id.
pub(crate) async fn rider_access(
    conn: &mut SqliteConnection,
    actor: &Actor,
    rider_id: RiderId,
) -> Result<ClubId, RegistrationError> {
    let club_id = sqlx::query_scalar!(
        r#"SELECT club_id as "club_id!: ClubId" FROM rider WHERE id = $1"#,
        rider_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    club_access(conn, actor, club_id).await?;
    Ok(club_id)
}

/// Checks that the actor may change the act: the organizer and users with
/// access to the club of one of its riders or, for groups, to the club that
/// created it.
pub(crate) async fn act_access(
    conn: &mut SqliteConnection,
    actor: &Actor,
    act_id: ActId,
) -> Result<(), RegistrationError> {
    let allowed = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM clubs
            WHERE (clubs.owner_id = ?2 OR clubs.id = ?3)
                AND (
                    clubs.id IN (
                        SELECT rider.club_id FROM act_participants
                        JOIN rider ON rider.id = act_participants.rider_id
                        WHERE act_participants.act_id = acts.id
                    )
                    OR clubs.id = acts.owner_club_id
                )
        ) as "allowed!: bool"
        FROM acts
        WHERE acts.id = ?1
        "#,
        act_id,
        actor.user_id,
        actor.club_id,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    if allowed || actor.manage_registrations {
        Ok(())
    } else {
        Err(RegistrationError::Forbidden)
    }
}

/// Fails if the club already registered a rider with this name, giving the
/// name as registered (it may differ in case). `except_id` excludes the rider
/// being edited.
pub(crate) async fn ensure_rider_name_is_free(
    conn: &mut SqliteConnection,
    club_id: ClubId,
    firstname: &str,
    lastname: &str,
    except_id: Option<RiderId>,
) -> Result<(), RegistrationError> {
    let names = sqlx::query!(
        "SELECT firstname, lastname FROM rider WHERE club_id = $1 AND id IS NOT $2",
        club_id,
        except_id,
    )
    .fetch_all(&mut *conn)
    .await?;
    let key = people::person_key(firstname, lastname);
    match names
        .iter()
        .find(|row| people::person_key(&row.firstname, &row.lastname) == key)
    {
        Some(row) => Err(RegistrationError::Rejected(
            people::already_registered_message(&row.firstname, &row.lastname),
        )),
        None => Ok(()),
    }
}

/// The database's own uniqueness rule, for the rare request that slips past
/// [`ensure_rider_name_is_free`] at the same moment: the same message.
pub(crate) fn map_duplicate(
    error: sqlx::Error,
    firstname: &str,
    lastname: &str,
) -> RegistrationError {
    if error
        .as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        RegistrationError::Rejected(people::already_registered_message(firstname, lastname))
    } else {
        error.into()
    }
}

/// The rider's single and pair starts.
pub(crate) async fn starts(conn: &mut SqliteConnection, rider: RiderId) -> sqlx::Result<Starts> {
    let single = act_of(conn, rider, ActType::Single).await?;
    let pair = match act_of(conn, rider, ActType::Pair).await? {
        Some(act_id) => Pair::Linked {
            act_id,
            partner: sqlx::query_scalar!(
                r#"SELECT rider_id as "rider_id!: RiderId" FROM act_participants WHERE act_id = $1 AND rider_id != $2"#,
                act_id,
                rider,
            )
            .fetch_optional(&mut *conn)
            .await?,
        },
        None => match sqlx::query_scalar!(
            "SELECT partner_name FROM pair_requests WHERE rider_id = $1",
            rider
        )
        .fetch_optional(&mut *conn)
        .await?
        {
            Some(partner_name) => Pair::Requested { partner_name },
            None => Pair::None,
        },
    };
    Ok(Starts { single, pair })
}

/// Adds or removes the rider's single. A new single needs the rider's gender,
/// which decides Einzel weiblich or männlich.
pub(crate) async fn set_single(
    conn: &mut SqliteConnection,
    rider: RiderId,
    single: bool,
) -> Result<(), RegistrationError> {
    let act_id = act_of(conn, rider, ActType::Single).await?;
    match (single, act_id) {
        (true, None) => {
            let rider = Rider::load(conn, rider).await?;
            if rider.gender.is_none() {
                return Err(gender_missing(&rider));
            }
            insert_act(conn, ActType::Single, &[rider.id]).await?;
        }
        (false, Some(act_id)) => delete_act(conn, act_id).await?,
        _ => {}
    }
    Ok(())
}

/// The rider waits for a pair partner, named or not. Within the club, a
/// waiting rider of that name who names this rider (or nobody) becomes the
/// partner at once. Returns that partner.
///
/// Fails if the rider already is in a pair.
pub(crate) async fn request_pair(
    conn: &mut SqliteConnection,
    rider: RiderId,
    partner_name: Option<&str>,
) -> Result<Option<RiderId>, RegistrationError> {
    let rider = Rider::load(conn, rider).await?;
    ensure_not_in_pair(conn, &rider).await?;
    let partner_name = partner_name.map(str::trim).filter(|name| !name.is_empty());
    if partner_name == Some(rider.name.as_str()) {
        return Err(same_person());
    }
    sqlx::query!(
        r#"
        INSERT INTO pair_requests (rider_id, partner_name) VALUES ($1, $2)
        ON CONFLICT (rider_id) DO UPDATE SET partner_name = excluded.partner_name
        "#,
        rider.id,
        partner_name,
    )
    .execute(&mut *conn)
    .await?;

    let Some(partner_name) = partner_name else {
        return Ok(None);
    };
    let candidates = sqlx::query_scalar!(
        r#"
        SELECT rider.id as "id!: RiderId"
        FROM pair_requests JOIN rider ON rider.id = pair_requests.rider_id
        WHERE rider.club_id = $1
            AND rider.id != $2
            AND concat_ws(' ', rider.firstname, rider.lastname) = $3
            AND (pair_requests.partner_name IS NULL OR pair_requests.partner_name = $4)
        "#,
        rider.club_id,
        rider.id,
        partner_name,
        rider.name,
    )
    .fetch_all(&mut *conn)
    .await?;
    let [partner] = candidates[..] else {
        return Ok(None);
    };
    link(conn, rider.id, partner).await?;
    Ok(Some(partner))
}

/// Links two riders to a pair act; their pair requests go. Fails if one of
/// them already is in a pair.
pub(crate) async fn link_pair(
    conn: &mut SqliteConnection,
    rider: RiderId,
    partner: RiderId,
    clubs: PartnerClubs,
) -> Result<ActId, RegistrationError> {
    if rider == partner {
        return Err(same_person());
    }
    let rider = Rider::load(conn, rider).await?;
    let partner = Rider::load(conn, partner).await?;
    if clubs == PartnerClubs::Same && rider.club_id != partner.club_id {
        return Err(RegistrationError::Rejected(
            "Die Paarkür-PartnerIn muss aus dem eigenen Verein sein. Vereinsübergreifende Paare verbindet der Veranstalter."
                .to_string(),
        ));
    }
    ensure_not_in_pair(conn, &rider).await?;
    ensure_not_in_pair(conn, &partner).await?;
    Ok(link(conn, rider.id, partner.id).await?)
}

/// Takes the rider out of the pair competition: their pair request goes, or
/// their pair act is dissolved and the partner is left with a pair request
/// without a name.
pub(crate) async fn leave_pair(
    conn: &mut SqliteConnection,
    rider: RiderId,
) -> Result<(), RegistrationError> {
    match starts(conn, rider).await?.pair {
        Pair::None => {}
        Pair::Requested { .. } => {
            sqlx::query!("DELETE FROM pair_requests WHERE rider_id = $1", rider)
                .execute(&mut *conn)
                .await?;
        }
        Pair::Linked { act_id, partner } => {
            delete_act(conn, act_id).await?;
            if let Some(partner) = partner {
                sqlx::query!(
                    "INSERT INTO pair_requests (rider_id, partner_name) VALUES ($1, NULL)",
                    partner
                )
                .execute(&mut *conn)
                .await?;
            }
        }
    }
    Ok(())
}

/// Dissolves a pair act: both riders leave the pair competition, nobody is
/// left waiting for a partner (unlike [`leave_pair`]).
pub(crate) async fn dissolve_pair(
    conn: &mut SqliteConnection,
    act_id: ActId,
) -> Result<(), RegistrationError> {
    let act_type = sqlx::query_scalar!(
        r#"SELECT act_type as "act_type!: ActType" FROM acts WHERE id = $1"#,
        act_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    if act_type != ActType::Pair {
        return Err(RegistrationError::Rejected(
            "Die Kür ist keine Paarkür.".to_string(),
        ));
    }
    Ok(delete_act(conn, act_id).await?)
}

/// Replaces a rider of a single or pair act by `new`, keeping the act with
/// its title, description and music. `new` must not have such an act yet;
/// their pair request is fulfilled by the pair act.
pub(crate) async fn replace_rider(
    conn: &mut SqliteConnection,
    act_id: ActId,
    old: RiderId,
    new: RiderId,
) -> Result<(), RegistrationError> {
    let act_type = sqlx::query_scalar!(
        r#"SELECT act_type as "act_type!: ActType" FROM acts WHERE id = $1"#,
        act_id
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(RegistrationError::NotFound)?;
    let new = Rider::load(conn, new).await?;
    match act_type {
        ActType::Single => {
            if act_of(conn, new.id, ActType::Single).await?.is_some() {
                return Err(RegistrationError::Rejected(format!(
                    "{} fährt schon eine Einzelkür.",
                    new.name
                )));
            }
            if new.gender.is_none() {
                return Err(gender_missing(&new));
            }
        }
        ActType::Pair => {
            ensure_not_in_pair(conn, &new).await?;
            sqlx::query!("DELETE FROM pair_requests WHERE rider_id = $1", new.id)
                .execute(&mut *conn)
                .await?;
        }
        ActType::GroupSmall | ActType::GroupLarge => {
            return Err(RegistrationError::Rejected(
                "Die Fahrer einer Gruppe werden in der Gruppe ausgewählt.".to_string(),
            ));
        }
    }
    let replaced = sqlx::query!(
        "UPDATE act_participants SET rider_id = $1 WHERE act_id = $2 AND rider_id = $3",
        new.id,
        act_id,
        old,
    )
    .execute(&mut *conn)
    .await?;
    if replaced.rows_affected() == 0 {
        return Err(RegistrationError::NotFound);
    }
    Ok(())
}

/// Deletes the rider with their single; their pair is dissolved (see
/// [`leave_pair`]) and their groups keep the other riders.
pub(crate) async fn delete_rider(
    conn: &mut SqliteConnection,
    rider: RiderId,
) -> Result<(), RegistrationError> {
    set_single(conn, rider, false).await?;
    leave_pair(conn, rider).await?;
    // Groups stay with their other riders and keep their type.
    sqlx::query!("DELETE FROM act_participants WHERE rider_id = $1", rider)
        .execute(&mut *conn)
        .await?;
    sqlx::query!("DELETE FROM rider WHERE id = $1", rider)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// What the rules need to know about a rider.
struct Rider {
    id: RiderId,
    club_id: ClubId,
    name: String,
    gender: Option<Gender>,
}

impl Rider {
    /// Fails with 404 for an unknown rider.
    async fn load(conn: &mut SqliteConnection, id: RiderId) -> Result<Rider, RegistrationError> {
        let row = sqlx::query!(
            r#"
            SELECT
                club_id as "club_id!: ClubId",
                concat_ws(' ', firstname, lastname) as "name!: String",
                gender as "gender: Gender"
            FROM rider WHERE id = $1
            "#,
            id
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(RegistrationError::NotFound)?;
        Ok(Rider {
            id,
            club_id: row.club_id,
            name: row.name,
            gender: row.gender,
        })
    }
}

/// A single needs the rider's gender: it's Einzel weiblich or männlich by it.
fn gender_missing(rider: &Rider) -> RegistrationError {
    RegistrationError::Rejected(format!(
        "Bitte gib zuerst das Geschlecht von {} an.",
        rider.name
    ))
}

fn same_person() -> RegistrationError {
    RegistrationError::Rejected("Fahrer und Partner sind die selbe Person!".to_string())
}

/// One pair per rider: fails if the rider is in a pair act.
async fn ensure_not_in_pair(
    conn: &mut SqliteConnection,
    rider: &Rider,
) -> Result<(), RegistrationError> {
    if act_of(conn, rider.id, ActType::Pair).await?.is_some() {
        return Err(RegistrationError::Rejected(format!(
            "{} fährt schon eine Paarkür.",
            rider.name
        )));
    }
    Ok(())
}

/// The pair act of two riders without one; their pair requests are fulfilled.
async fn link(
    conn: &mut SqliteConnection,
    rider: RiderId,
    partner: RiderId,
) -> sqlx::Result<ActId> {
    sqlx::query!(
        "DELETE FROM pair_requests WHERE rider_id IN ($1, $2)",
        rider,
        partner
    )
    .execute(&mut *conn)
    .await?;
    insert_act(conn, ActType::Pair, &[rider, partner]).await
}

/// The rider's act of a single or pair type.
async fn act_of(
    conn: &mut SqliteConnection,
    rider: RiderId,
    act_type: ActType,
) -> sqlx::Result<Option<ActId>> {
    sqlx::query_scalar!(
        r#"
        SELECT act_id as "act_id!: ActId"
        FROM act_participants JOIN acts ON acts.id = act_participants.act_id
        WHERE act_participants.rider_id = $1 AND acts.act_type = $2
        "#,
        rider,
        act_type
    )
    .fetch_optional(&mut *conn)
    .await
}

async fn insert_act(
    conn: &mut SqliteConnection,
    act_type: ActType,
    riders: &[RiderId],
) -> sqlx::Result<ActId> {
    let id = ActId::new();
    sqlx::query!(
        "INSERT INTO acts (id, name, act_type) VALUES ($1, '', $2)",
        id,
        act_type
    )
    .execute(&mut *conn)
    .await?;
    for rider in riders {
        sqlx::query!(
            "INSERT INTO act_participants (act_id, rider_id) VALUES ($1, $2)",
            id,
            rider
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(id)
}

/// Deletes the act; its participants go with it.
async fn delete_act(conn: &mut SqliteConnection, act_id: ActId) -> sqlx::Result<()> {
    sqlx::query!("DELETE FROM acts WHERE id = $1", act_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn insert_club(db: &SqlitePool, name: &str) -> ClubId {
        let owner_id = UserId::new();
        sqlx::query(
            "INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, $2, true, $3, 'x')",
        )
        .bind(owner_id)
        .bind(format!("{owner_id}@example.com"))
        .bind(name)
        .execute(db)
        .await
        .unwrap();
        let club_id = ClubId::new();
        sqlx::query("INSERT INTO clubs (id, name, owner_id) VALUES ($1, $2, $3)")
            .bind(club_id)
            .bind(name)
            .bind(owner_id)
            .execute(db)
            .await
            .unwrap();
        club_id
    }

    /// A rider without starts, named `{firstname} {lastname}`.
    async fn insert_rider(db: &SqlitePool, club_id: ClubId, name: &str) -> RiderId {
        let (firstname, lastname) = name.split_once(' ').unwrap();
        let id = RiderId::new();
        sqlx::query(
            "INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender) VALUES ($1, $2, $3, $4, '2012-05-01T00:00:00Z', 'female')",
        )
        .bind(id)
        .bind(club_id)
        .bind(firstname)
        .bind(lastname)
        .execute(db)
        .await
        .unwrap();
        id
    }

    async fn pair_of(db: &SqlitePool, rider: RiderId) -> Pair {
        starts(&mut db.acquire().await.unwrap(), rider)
            .await
            .unwrap()
            .pair
    }

    fn message(error: RegistrationError) -> String {
        match error {
            RegistrationError::Rejected(message) => message,
            other => panic!("not a message: {other:?}"),
        }
    }

    #[tokio::test]
    async fn single_on_and_off() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let mut conn = db.acquire().await.unwrap();
        assert_eq!(starts(&mut conn, anna).await.unwrap().single, None);

        set_single(&mut conn, anna, true).await.unwrap();
        let single = starts(&mut conn, anna).await.unwrap().single;
        assert!(single.is_some());

        // Setting it again keeps the act (and its music).
        set_single(&mut conn, anna, true).await.unwrap();
        assert_eq!(starts(&mut conn, anna).await.unwrap().single, single);

        set_single(&mut conn, anna, false).await.unwrap();
        assert_eq!(starts(&mut conn, anna).await.unwrap().single, None);
    }

    #[tokio::test]
    async fn waiting_riders_naming_each_other_become_a_pair() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let mut conn = db.acquire().await.unwrap();

        assert_eq!(
            request_pair(&mut conn, anna, Some("Berta Muster"))
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            pair_of(&db, anna).await,
            Pair::Requested {
                partner_name: Some("Berta Muster".to_string())
            }
        );

        assert_eq!(
            request_pair(&mut conn, berta, Some("Anna Muster"))
                .await
                .unwrap(),
            Some(anna)
        );
        let Pair::Linked { act_id, partner } = pair_of(&db, anna).await else {
            panic!("not linked");
        };
        assert_eq!(partner, Some(berta));
        assert_eq!(
            pair_of(&db, berta).await,
            Pair::Linked {
                act_id,
                partner: Some(anna)
            }
        );
    }

    #[tokio::test]
    async fn a_newcomer_naming_a_waiting_rider_pairs_with_them() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let mut conn = db.acquire().await.unwrap();
        request_pair(&mut conn, anna, None).await.unwrap();

        assert_eq!(
            request_pair(&mut conn, berta, Some(" Anna Muster "))
                .await
                .unwrap(),
            Some(anna)
        );
        assert!(matches!(pair_of(&db, anna).await, Pair::Linked { .. }));
    }

    #[tokio::test]
    async fn a_waiting_rider_naming_someone_else_or_of_another_club_isnt_taken() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let other_club = insert_club(&db, "ESV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let zoe = insert_rider(&db, other_club, "Zoe Muster").await;
        let mut conn = db.acquire().await.unwrap();
        request_pair(&mut conn, anna, Some("Clara Muster"))
            .await
            .unwrap();
        request_pair(&mut conn, zoe, Some("Berta Muster"))
            .await
            .unwrap();

        assert_eq!(
            request_pair(&mut conn, berta, Some("Anna Muster"))
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            request_pair(&mut conn, berta, Some("Zoe Muster"))
                .await
                .unwrap(),
            None
        );
        assert!(matches!(pair_of(&db, zoe).await, Pair::Requested { .. }));
    }

    #[tokio::test]
    async fn a_paired_rider_is_never_taken() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let max = insert_rider(&db, club, "Max Müller").await;
        let lena = insert_rider(&db, club, "Lena Schmidt").await;
        let mia = insert_rider(&db, club, "Mia Weber").await;
        let mut conn = db.acquire().await.unwrap();
        request_pair(&mut conn, max, Some("Lena Schmidt"))
            .await
            .unwrap();
        request_pair(&mut conn, lena, Some("Max Müller"))
            .await
            .unwrap();

        assert_eq!(
            request_pair(&mut conn, mia, Some("Lena Schmidt"))
                .await
                .unwrap(),
            None
        );

        assert!(matches!(
            pair_of(&db, max).await,
            Pair::Linked { partner: Some(partner), .. } if partner == lena
        ));
        assert_eq!(
            pair_of(&db, mia).await,
            Pair::Requested {
                partner_name: Some("Lena Schmidt".to_string())
            }
        );
    }

    #[tokio::test]
    async fn a_rider_cant_ask_for_themselves_or_a_second_pair() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let mut conn = db.acquire().await.unwrap();

        let error = request_pair(&mut conn, anna, Some("Anna Muster"))
            .await
            .unwrap_err();
        assert!(message(error).contains("selbe Person"));

        link_pair(&mut conn, anna, berta, PartnerClubs::Same)
            .await
            .unwrap();
        let error = request_pair(&mut conn, anna, None).await.unwrap_err();
        assert!(message(error).contains("schon eine Paarkür"));
    }

    #[tokio::test]
    async fn the_organizer_links_riders_of_different_clubs() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let other_club = insert_club(&db, "ESV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let zoe = insert_rider(&db, other_club, "Zoe Muster").await;
        let yva = insert_rider(&db, other_club, "Yva Muster").await;
        let mut conn = db.acquire().await.unwrap();
        request_pair(&mut conn, anna, Some("Zoe Muster"))
            .await
            .unwrap();
        request_pair(&mut conn, zoe, Some("Anna Muster"))
            .await
            .unwrap();
        request_pair(&mut conn, berta, Some("Yva")).await.unwrap();

        let error = link_pair(&mut conn, anna, zoe, PartnerClubs::Same)
            .await
            .unwrap_err();
        assert!(message(error).contains("eigenen Verein"));

        // Both asked for each other …
        let act_id = link_pair(&mut conn, anna, zoe, PartnerClubs::Any)
            .await
            .unwrap();
        assert_eq!(
            pair_of(&db, zoe).await,
            Pair::Linked {
                act_id,
                partner: Some(anna)
            }
        );
        // … or only one of them did.
        link_pair(&mut conn, berta, yva, PartnerClubs::Any)
            .await
            .unwrap();
        assert!(matches!(
            pair_of(&db, berta).await,
            Pair::Linked { partner: Some(partner), .. } if partner == yva
        ));
        let requests: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pair_requests")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(requests, 0);

        let error = link_pair(&mut conn, anna, yva, PartnerClubs::Any)
            .await
            .unwrap_err();
        assert!(message(error).contains("schon eine Paarkür"));
    }

    #[tokio::test]
    async fn leaving_a_pair_leaves_the_partner_waiting() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let mut conn = db.acquire().await.unwrap();
        link_pair(&mut conn, anna, berta, PartnerClubs::Same)
            .await
            .unwrap();

        leave_pair(&mut conn, anna).await.unwrap();

        assert_eq!(pair_of(&db, anna).await, Pair::None);
        assert_eq!(
            pair_of(&db, berta).await,
            Pair::Requested { partner_name: None }
        );
        let acts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM acts")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(acts, 0);

        // A waiting rider just stops waiting.
        leave_pair(&mut conn, berta).await.unwrap();
        assert_eq!(pair_of(&db, berta).await, Pair::None);
    }

    #[tokio::test]
    async fn deleting_a_rider_of_a_pair_leaves_the_partner_waiting() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let mut conn = db.acquire().await.unwrap();
        set_single(&mut conn, anna, true).await.unwrap();
        link_pair(&mut conn, anna, berta, PartnerClubs::Same)
            .await
            .unwrap();

        delete_rider(&mut conn, anna).await.unwrap();

        assert_eq!(
            pair_of(&db, berta).await,
            Pair::Requested { partner_name: None }
        );
        let riders: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rider")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(riders, 1);
    }

    #[tokio::test]
    async fn dissolving_a_pair_leaves_nobody_waiting() {
        let db = crate::reloadable_sqlite::in_memory_migrated().await;
        let club = insert_club(&db, "RSV").await;
        let anna = insert_rider(&db, club, "Anna Muster").await;
        let berta = insert_rider(&db, club, "Berta Muster").await;
        let mut conn = db.acquire().await.unwrap();
        let act_id = link_pair(&mut conn, anna, berta, PartnerClubs::Same)
            .await
            .unwrap();
        set_single(&mut conn, anna, true).await.unwrap();
        let single = starts(&mut conn, anna).await.unwrap().single.unwrap();

        dissolve_pair(&mut conn, act_id).await.unwrap();

        assert_eq!(pair_of(&db, anna).await, Pair::None);
        assert_eq!(pair_of(&db, berta).await, Pair::None);
        // Only pair acts are dissolved.
        let error = dissolve_pair(&mut conn, single).await.unwrap_err();
        assert!(message(error).contains("keine Paarkür"));
    }
}
