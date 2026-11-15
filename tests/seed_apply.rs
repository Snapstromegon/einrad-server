//! Applying and importing the competition file: what it writes, replaces and
//! drops, and what it leaves alone (ADR 0005).

use std::path::Path;

use db_core::CompetitionRepo;
use db_sqlite::SqliteRepo;
use einrad_server::{
    config::CompetitionFile,
    jury::JudgeTask,
    seed::{self, Applied, Dropped, RemapFile, RemapRegistration, Report},
};
use time::macros::date;

const DAY: time::Date = date!(2027 - 06 - 01);

const COMPETITION: &str = r#"
    [competition]
    id = "cup"
    name = "Cup"
    location = "Halle"
    start_date = 2027-06-05
    end_date = 2027-06-06
    determining_day = 2027-06-01
    registration_start = 2026-11-01T00:00:00+01:00
    registration_end = 2027-04-30T23:59:59+02:00
    music_upload_end = 2027-05-20T23:59:59+02:00
    fee_per_start = 15

    [[categories]]
    name = "U15"
    act_type = "single"
    under_age = 15

    [[categories]]
    name = "15+"
    act_type = "single"
    min_age = 15

    [[timeplan]]
    category = "U15"
    earliest_start = 2027-06-05T10:00:00+02:00

    [[timeplan]]
    label = "Pause"
    duration_minutes = 30

    [[timeplan]]
    category = "15+"
"#;

/// Directory removed again when dropped.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cup-seed-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn apply_file(
    db: &sqlx::SqlitePool,
    toml: &str,
    replace: bool,
    data_path: &Path,
) -> Result<Report, String> {
    let file = CompetitionFile::parse(toml).unwrap();
    seed::apply(
        &SqliteRepo::from_pool(db.clone()),
        Some(&file),
        replace,
        data_path,
    )
    .await
}

async fn seed_file(db: &sqlx::SqlitePool, toml: &str) -> Result<(), String> {
    let data = TempDir::new();
    apply_file(db, toml, false, &data.0).await.map(|_| ())
}

async fn count(db: &sqlx::SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(db)
        .await
        .unwrap()
}

#[tokio::test]
async fn migrations_leave_tables_empty() {
    let db = db_inmemory::pool().await;
    for table in [
        "competition",
        "categories",
        "timeplan",
        "users",
        "clubs",
        "rider",
        "acts",
    ] {
        assert_eq!(count(&db, table).await, 0, "{table}");
    }
}

#[tokio::test]
async fn a_changed_file_on_the_next_start_changes_nothing_and_warns() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    let created = apply_file(&db, COMPETITION, false, &data.0).await.unwrap();
    assert_eq!(created.applied, Applied::Created);
    assert!(created.warnings.is_empty(), "{:?}", created.warnings);

    let unchanged = apply_file(&db, COMPETITION, false, &data.0).await.unwrap();
    assert_eq!(unchanged.applied, Applied::Unchanged);
    assert!(unchanged.warnings.is_empty(), "{:?}", unchanged.warnings);

    let changed = COMPETITION.replace("name = \"Cup\"", "name = \"Cup 2\"");
    let report = apply_file(&db, &changed, false, &data.0).await.unwrap();
    assert_eq!(report.applied, Applied::Ignored);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(
        report.warnings[0].contains("the competition settings"),
        "{:?}",
        report.warnings
    );
    assert!(
        report.warnings[0].contains("--replace-competition"),
        "{:?}",
        report.warnings
    );
    assert_eq!(
        SqliteRepo::from_pool(db.clone())
            .current_competition()
            .await
            .unwrap()
            .unwrap()
            .name,
        "Cup"
    );
}

#[tokio::test]
async fn any_changed_detail_counts_as_a_difference() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    apply_file(&db, &planned_competition(), false, &data.0)
        .await
        .unwrap();
    let cases = [
        ("under_age = 15", "under_age = 14", "the categories"),
        ("count = 2", "count = 3", "the finals"),
        (
            "duration_minutes = 30",
            "duration_minutes = 45",
            "the timeplan",
        ),
        (
            "name = \"Vorlauf\"",
            "name = \"Vorlauf\"\n    jury_deployments = 1",
            "the heats",
        ),
        (
            "name = \"Finale\"\n    act_type",
            "name = \"Finale\"\n    jury_registrable = false\n    act_type",
            "the finals",
        ),
    ];
    for (from, to, expected) in cases {
        let changed = planned_competition().replace(from, to);
        let report = apply_file(&db, &changed, false, &data.0).await.unwrap();
        assert_eq!(report.applied, Applied::Ignored, "{to}");
        assert!(
            report.warnings.len() == 1 && report.warnings[0].contains(expected),
            "{to}: {:?}",
            report.warnings
        );
    }
}

#[tokio::test]
async fn an_invalid_file_on_the_next_start_is_ignored_with_a_warning() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    apply_file(&db, COMPETITION, false, &data.0).await.unwrap();

    let invalid = COMPETITION.replace("category = \"15+\"", "category = \"missing\"");
    let report = apply_file(&db, &invalid, false, &data.0).await.unwrap();
    assert_eq!(report.applied, Applied::Ignored);
    assert!(
        report.warnings.len() == 1 && report.warnings[0].contains("missing"),
        "{:?}",
        report.warnings
    );
    assert_eq!(count(&db, "timeplan").await, 3);
}

#[tokio::test]
async fn replacing_with_an_unchanged_file_keeps_the_planning() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    apply_file(&db, &planned_competition(), false, &data.0)
        .await
        .unwrap();
    add_club_data_and_planning(&db).await;

    let report = apply_file(&db, &planned_competition(), true, &data.0)
        .await
        .unwrap();
    assert_eq!(report.applied, Applied::Unchanged);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(count(&db, "timeplan_slots").await, 1);
    assert_eq!(count(&db, "jury_assignments").await, 1);
}

#[tokio::test]
async fn a_file_with_another_id_is_ignored_with_a_warning() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    apply_file(&db, COMPETITION, false, &data.0).await.unwrap();

    let renamed = COMPETITION.replace(r#"id = "cup""#, r#"id = "cup-2""#);
    let report = apply_file(&db, &renamed, false, &data.0).await.unwrap();
    assert_eq!(report.applied, Applied::Ignored);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(
        report.warnings[0].contains("id cup-2")
            && report.warnings[0].contains("cup")
            && report.warnings[0].contains("never changes"),
        "{:?}",
        report.warnings
    );
    assert_eq!(count(&db, "competition").await, 1);
    assert_eq!(
        SqliteRepo::from_pool(db.clone())
            .current_competition()
            .await
            .unwrap()
            .unwrap()
            .id,
        "cup"
    );
}

/// [`COMPETITION`] with a heat and a final, the two jury units.
fn planned_competition() -> String {
    format!(
        r#"{COMPETITION}
    [[timeplan]]
    final = "Finale"

    [[heats]]
    name = "Vorlauf"
    categories = ["U15"]

    [[finals]]
    name = "Finale"
    act_type = "single"
    slots = [{{ heat = "Vorlauf", count = 2 }}]
    "#
    )
}

/// A club with a rider, an act, a judge registered for both jury
/// units and a booking, plus planning: the act in a timeplan slot
/// and a finals slot and already performed, the judge in the published
/// jury plan.
async fn add_club_data_and_planning(db: &sqlx::SqlitePool) {
    let (user, club, rider, act, judge) = (
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    let heat_id =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM heats WHERE name = 'Vorlauf'")
            .fetch_one(db)
            .await
            .unwrap();
    let final_id =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM finals WHERE name = 'Finale'")
            .fetch_one(db)
            .await
            .unwrap();
    let statements: Vec<sqlx::query::Query<'_, sqlx::Sqlite, _>> = vec![
        sqlx::query("INSERT INTO users (id, email, email_verified, name, password) VALUES ($1, 'max@example.org', true, 'Max', '')")
            .bind(user),
        sqlx::query("INSERT INTO clubs (id, name, owner_id) VALUES ($1, 'Club A', $2)")
            .bind(club)
            .bind(user),
        sqlx::query("INSERT INTO rider (id, club_id, firstname, lastname, birthdate, gender) VALUES ($1, $2, 'Erika', 'Muster', '2014-01-01T00:00:00Z', 'female')")
            .bind(rider)
            .bind(club),
        sqlx::query("INSERT INTO acts (id, name, act_type, song_file, started_at, ended_at) VALUES ($1, 'Kür', 'single', 'song.mp3', '2027-06-05T10:00:00Z', '2027-06-05T10:04:00Z')").bind(act),
        sqlx::query("INSERT INTO act_participants (act_id, rider_id) VALUES ($1, $2)")
            .bind(act)
            .bind(rider),
        sqlx::query("INSERT INTO judge (id, club_id, firstname, lastname, birthdate, is_trained) VALUES ($1, $2, 'Max', 'Müller', '1990-01-01T00:00:00Z', true)")
            .bind(judge)
            .bind(club),
        sqlx::query("INSERT INTO judge_registration (judge_id, heat_id, task) VALUES ($1, $2, 'technical')")
            .bind(judge)
            .bind(heat_id),
        sqlx::query("INSERT INTO judge_registration (judge_id, final_id, task) VALUES ($1, $2, 'performance')")
            .bind(judge)
            .bind(final_id),
        sqlx::query("INSERT INTO jury_assignments (judge_id, heat_id, task) VALUES ($1, $2, 'technical')")
            .bind(judge)
            .bind(heat_id),
        sqlx::query("UPDATE competition SET jury_plan_published_at = '2027-01-01T00:00:00Z'"),
        sqlx::query("INSERT INTO timeplan_slots (timeplan_id, position, act_id) VALUES ((SELECT MIN(id) FROM timeplan), 1, $1)")
            .bind(act),
        sqlx::query("UPDATE final_slots SET act_id = $1 WHERE position = 1").bind(act),
        sqlx::query("INSERT INTO bookings (id, amount, date, club_id) VALUES ($1, 15, '2027-01-01', $2)")
            .bind(uuid::Uuid::now_v7())
            .bind(club),
    ];
    for statement in statements {
        statement.execute(db).await.unwrap();
    }
}

#[tokio::test]
async fn replace_keeps_club_data_and_drops_the_planning() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    apply_file(&db, &planned_competition(), false, &data.0)
        .await
        .unwrap();
    add_club_data_and_planning(&db).await;

    let replacement = planned_competition()
        .replace("name = \"Cup\"", "name = \"Cup 2\"")
        .replace("Finale", "Endlauf");
    let report = apply_file(&db, &replacement, true, &data.0).await.unwrap();

    let Applied::Replaced(dropped) = report.applied else {
        panic!("{:?}", report.applied);
    };
    assert_eq!(dropped.timeplan_slots, 1);
    assert_eq!(dropped.act_progress, 1);
    assert_eq!(dropped.finals_placements, 1);
    assert_eq!(dropped.jury_assignments, 1);
    assert!(dropped.jury_plan_published);
    let unmatched = vec![RemapRegistration {
        club: "Club A".into(),
        firstname: "Max".into(),
        lastname: "Müller".into(),
        task: JudgeTask::Performance,
        old_jury_unit: "final: Finale".into(),
        jury_unit: String::new(),
    }];
    assert_eq!(dropped.judge_registrations, unmatched);
    let remap_file = dropped.remap_file.expect("a remap file");
    assert!(
        remap_file.starts_with(data.0.join("remap")),
        "{remap_file:?}"
    );
    let name = remap_file.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("jury-registrations-") && name.ends_with(".toml"),
        "{name}"
    );
    let written: RemapFile =
        toml::from_str(&std::fs::read_to_string(&remap_file).unwrap()).unwrap();
    assert_eq!(written.jury_registrations, unmatched);
    assert!(
        report.warnings.iter().any(|w| w.contains("1 timeplan slot")
            && w.contains("the progress of 1 act")
            && w.contains("1 finals placement")
            && w.contains("the published jury plan")
            && w.contains("1 judge registration")),
        "{:?}",
        report.warnings
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains(&remap_file.display().to_string())),
        "{:?}",
        report.warnings
    );

    let competition = SqliteRepo::from_pool(db.clone())
        .current_competition()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (competition.id.as_str(), competition.name.as_str()),
        ("cup", "Cup 2")
    );
    for (table, expected) in [
        ("users", 1),
        ("clubs", 1),
        ("rider", 1),
        ("acts", 1),
        ("act_participants", 1),
        ("judge", 1),
        ("bookings", 1),
        ("timeplan_slots", 0),
        ("jury_assignments", 0),
        ("judge_registration", 1),
    ] {
        assert_eq!(count(&db, table).await, expected, "{table}");
    }
    let kept: (String, String) = sqlx::query_as(
        "SELECT view_jury_unit.name, task FROM judge_registration JOIN view_jury_unit ON view_jury_unit.id = unit_id",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(kept, ("Vorlauf".to_string(), "technical".to_string()));
    let progress: (Option<String>, Option<String>) =
        sqlx::query_as("SELECT started_at, ended_at FROM acts")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(progress, (None, None));
    let placements: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM final_slots WHERE act_id IS NOT NULL")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(placements, 0);
    let published: Option<String> =
        sqlx::query_scalar("SELECT jury_plan_published_at FROM competition")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(published, None);
}

#[tokio::test]
async fn replace_keeps_the_database_id_and_warns() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    apply_file(&db, COMPETITION, false, &data.0).await.unwrap();

    let renamed = COMPETITION
        .replace(r#"id = "cup""#, r#"id = "cup-2""#)
        .replace("name = \"Cup\"", "name = \"Cup 2\"");
    let report = apply_file(&db, &renamed, true, &data.0).await.unwrap();
    assert_eq!(report.applied, Applied::Replaced(Dropped::default()));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("id cup-2") && w.contains("keeps cup")),
        "{:?}",
        report.warnings
    );
    assert_eq!(count(&db, "competition").await, 1);
    let competition = SqliteRepo::from_pool(db.clone())
        .current_competition()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (competition.id.as_str(), competition.name.as_str()),
        ("cup", "Cup 2")
    );
}

#[tokio::test]
async fn an_invalid_replacement_changes_nothing() {
    let db = db_inmemory::pool().await;
    let data = TempDir::new();
    apply_file(&db, &planned_competition(), false, &data.0)
        .await
        .unwrap();
    add_club_data_and_planning(&db).await;

    let invalid = planned_competition()
        .replace("Finale", "Endlauf")
        .replace("category = \"15+\"", "category = \"missing\"");
    let err = apply_file(&db, &invalid, true, &data.0).await.unwrap_err();
    assert!(err.contains("missing"), "{err}");
    assert_eq!(count(&db, "timeplan_slots").await, 1);
    assert_eq!(count(&db, "jury_assignments").await, 1);
    assert_eq!(count(&db, "judge_registration").await, 2);
    assert!(!data.0.join("remap").exists());
}

#[tokio::test]
async fn seeds_empty_database() {
    let db = db_inmemory::pool().await;
    seed_file(&db, COMPETITION).await.unwrap();

    let competition = SqliteRepo::from_pool(db.clone())
        .current_competition()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(competition.determining_day, DAY);
    assert_eq!(competition.max_fee_per_rider, None);
    let categories: Vec<(String, i64)> =
        sqlx::query_as(r#"SELECT name, "order" FROM categories ORDER BY "order""#)
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        categories,
        vec![("U15".to_string(), 1), ("15+".to_string(), 2)]
    );
    assert_eq!(count(&db, "timeplan").await, 3);
}

#[tokio::test]
async fn invalid_competition_dates_write_nothing() {
    let db = db_inmemory::pool().await;
    let broken = COMPETITION.replace(
        "registration_end = 2027-04-30T23:59:59+02:00",
        "registration_end = 2027-12-20T23:59:59+01:00",
    );
    let err = seed_file(&db, &broken).await.unwrap_err();
    assert!(err.contains("registration_end"), "{err}");
    assert_eq!(count(&db, "competition").await, 0);
}

#[tokio::test]
async fn invalid_config_writes_nothing() {
    let db = db_inmemory::pool().await;
    let invalid = COMPETITION.replace("category = \"15+\"", "category = \"missing\"");
    let err = seed_file(&db, &invalid).await.unwrap_err();
    assert!(err.contains("missing"), "{err}");
    assert_eq!(count(&db, "competition").await, 0);
    assert_eq!(count(&db, "categories").await, 0);
    assert_eq!(count(&db, "timeplan").await, 0);
}

/// The competition with heats and finals appended.
fn with_heats(heats_and_finals: &str) -> String {
    format!(
        r#"{COMPETITION}
    [[categories]]
    name = "PU15"
    act_type = "pair"
    under_age = 15

    [[categories]]
    name = "M15"
    act_type = "single"
    single_male = true
    under_age = 15

    [[categories]]
    name = "KG"
    act_type = "group_small"

    {heats_and_finals}
    "#
    )
}

#[tokio::test]
async fn heats_are_seeded_with_their_categories() {
    let db = db_inmemory::pool().await;
    let file = with_heats(
        r#"
        [[heats]]
        name = "Junior"
        description = "Junior Expert"
        categories = ["U15"]

        [[heats]]
        name = "Expert"
        categories = ["15+"]

        [[finals]]
        name = "Finale"
        act_type = "single"
        slots = [{ heat = "Junior", count = 2 }, { heat = "Expert", count = 1 }]
        "#,
    );
    seed_file(&db, &file).await.unwrap();
    let heats: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT name, description FROM heats ORDER BY name")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        heats,
        [
            ("Expert".to_string(), None),
            ("Junior".to_string(), Some("Junior Expert".to_string())),
        ]
    );
    let linked: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT categories.name, heats.name FROM categories \
             LEFT JOIN heats ON heats.id = categories.heat_id ORDER BY categories.\"order\"",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(
        linked,
        [
            ("U15".to_string(), Some("Junior".to_string())),
            ("15+".to_string(), Some("Expert".to_string())),
            ("PU15".to_string(), None),
            ("M15".to_string(), None),
            ("KG".to_string(), None),
        ]
    );
    let slots: Vec<String> = sqlx::query_scalar(
        "SELECT heats.name FROM final_slots \
             JOIN heats ON heats.id = final_slots.heat_id ORDER BY position",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(slots, ["Junior", "Junior", "Expert"]);
}

const NAMED_HEATS: &str = r#"
    [[heats]]
    name = "Einzel – Junior (U15)"
    code = "J"
    categories = ["U15"]

    [[heats]]
    name = "Einzel – Expert (15+)"
    categories = ["15+"]

    [[finals]]
    name = "Finale Einzel"
    code = "FE"
    act_type = "single"
    slots = [{ heat = "J", count = 2 }, { heat = "Einzel – Expert (15+)", count = 1 }]

    [[timeplan]]
    final = "FE"
"#;

#[tokio::test]
async fn heats_and_finals_take_name_and_code_and_files_refer_to_the_code() {
    let db = db_inmemory::pool().await;
    seed_file(&db, &with_heats(NAMED_HEATS)).await.unwrap();

    let heats: Vec<(String, String)> = sqlx::query_as("SELECT code, name FROM heats ORDER BY code")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(
        heats,
        [
            // Without a code, the name is the code.
            (
                "Einzel – Expert (15+)".to_string(),
                "Einzel – Expert (15+)".to_string()
            ),
            ("J".to_string(), "Einzel – Junior (U15)".to_string()),
        ]
    );
    let finals: Vec<(String, String)> = sqlx::query_as("SELECT code, name FROM finals")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(finals, [("FE".to_string(), "Finale Einzel".to_string())]);
    let planned: Vec<String> = sqlx::query_scalar(
        "SELECT finals.name FROM timeplan JOIN finals ON finals.id = timeplan.final_id",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(planned, ["Finale Einzel"]);
}

#[tokio::test]
async fn references_name_the_code_not_the_readable_name() {
    for (from, to, message) in [
        (
            "heat = \"J\"",
            "heat = \"Einzel – Junior (U15)\"",
            "Einzel – Junior (U15)",
        ),
        (
            "final = \"FE\"",
            "final = \"Finale Einzel\"",
            "Finale Einzel",
        ),
    ] {
        let db = db_inmemory::pool().await;
        let file = with_heats(&NAMED_HEATS.replace(from, to));
        let err = seed_file(&db, &file).await.unwrap_err();
        assert!(err.contains(message), "{err}");
    }
}

#[tokio::test]
async fn codes_must_be_unique() {
    let db = db_inmemory::pool().await;
    let file = with_heats(&NAMED_HEATS.replace(
        "name = \"Einzel – Expert (15+)\"",
        "name = \"Einzel – Expert (15+)\"\ncode = \"J\"",
    ));
    let err = seed_file(&db, &file).await.unwrap_err();
    assert!(err.contains("Heat J"), "{err}");
}

#[tokio::test]
async fn heats_take_their_threshold_and_skip_from_the_file() {
    let db = db_inmemory::pool().await;
    let file = with_heats(
        r#"
        [[heats]]
        name = "Junior"
        categories = ["U15"]

        [[heats]]
        name = "Expert"
        categories = ["15+"]

        [[heats]]
        name = "Paare"
        categories = ["PU15"]
        threshold = 3
        skip = true
        "#,
    );
    seed_file(&db, &file).await.unwrap();
    let heats: Vec<(String, i64, bool)> =
        sqlx::query_as("SELECT name, threshold, skip FROM heats ORDER BY name")
            .fetch_all(&db)
            .await
            .unwrap();
    // Without a threshold: 6 for U15 and younger, otherwise 5.
    assert_eq!(
        heats,
        [
            ("Expert".to_string(), 5, false),
            ("Junior".to_string(), 6, false),
            ("Paare".to_string(), 3, true),
        ]
    );
}

#[tokio::test]
async fn heats_of_groups_skip_their_vorlauf_unless_the_file_says_otherwise() {
    let db = db_inmemory::pool().await;
    let file = with_heats(
        r#"
        [[heats]]
        name = "Kleingruppe"
        categories = ["KG"]

        [[finals]]
        name = "Finale KG"
        act_type = "group_small"
        slots = [{ heat = "Kleingruppe", count = 1 }]
        "#,
    );
    seed_file(&db, &file).await.unwrap();
    let skip: bool = sqlx::query_scalar("SELECT skip FROM heats WHERE name = 'Kleingruppe'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(skip);

    let db = db_inmemory::pool().await;
    seed_file(
        &db,
        &file.replace(
            "categories = [\"KG\"]",
            "categories = [\"KG\"]\nskip = false",
        ),
    )
    .await
    .unwrap();
    let skip: bool = sqlx::query_scalar("SELECT skip FROM heats WHERE name = 'Kleingruppe'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(!skip);
    // The export gives the default back as no skip key.
    let export = einrad_server::export::export(&SqliteRepo::from_pool(db.clone()))
        .await
        .unwrap()
        .unwrap();
    let heat = export
        .file
        .heats
        .iter()
        .find(|h| h.name == "Kleingruppe")
        .unwrap();
    assert_eq!(heat.skip, Some(false));
}

#[tokio::test]
async fn the_import_checks_heats_and_names_the_offender() {
    let heat = |name: &str, categories: &str| {
        format!("[[heats]]\nname = \"{name}\"\ncategories = [{categories}]\n")
    };
    let single_final = |heat: &str| {
        format!(
            "[[finals]]\nname = \"Finale\"\nact_type = \"single\"\nslots = [{{ heat = \"{heat}\", count = 1 }}]\n"
        )
    };
    for (file, message) in [
        (heat(" ", "\"U15\""), "empty name"),
        (heat("A", "\"U15\"") + &heat("A", "\"15+\""), "Heat A"),
        (heat("A", "\"U99\""), "U99"),
        // One act type and gender.
        (heat("A", "\"U15\", \"PU15\""), "PU15"),
        (heat("A", "\"U15\", \"M15\""), "M15"),
        // A category belongs to at most one heat.
        (heat("A", "\"U15\"") + &heat("B", "\"U15\""), "U15"),
        (heat("A", "\"U15\"") + "threshold = -1\n", "Heat A"),
        // A heat fills the slots of at most one final.
        (
            heat("A", "\"U15\"")
                + &single_final("A")
                + &single_final("A").replace("\"Finale\"", "\"Finale 2\""),
            "Final Finale 2",
        ),
        // A slot group names a heat of the file, of the final's discipline.
        (single_final("Fehlt"), "Fehlt"),
        (heat("A", "\"PU15\"") + &single_final("A"), "Final Finale"),
        (
            heat("A", "\"U15\"")
                + "[[finals]]\nname = \"Gruppe\"\nact_type = \"group_small\"\nslots = [{ heat = \"A\", count = 1 }]\n",
            "Final Gruppe",
        ),
    ] {
        let db = db_inmemory::pool().await;
        let err = seed_file(&db, &with_heats(&file)).await.unwrap_err();
        assert!(err.contains(message), "{message}: {err}");
        assert_eq!(count(&db, "competition").await, 0);
    }
}

#[tokio::test]
async fn age_uses_determining_day_of_competition() {
    let db = db_inmemory::pool().await;
    seed_file(&db, COMPETITION).await.unwrap();
    let age: f64 = sqlx::query_scalar(
        "SELECT (julianday((SELECT determining_day FROM competition ORDER BY start_date DESC LIMIT 1)) - julianday('2012-06-01')) / 365.2425",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert!((age - 15.0).abs() < 0.01, "{age}");
}
