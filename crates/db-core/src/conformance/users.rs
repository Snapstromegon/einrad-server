//! Users and authentication through the trait: registering, logging in,
//! verifying the email, password resets and roles.

use uuid::Uuid;

use super::{World, admin};
use crate::{
    Db,
    actor::Actor,
    commands::user::{
        Login, Register, RequestPasswordReset, ResendMailValidation, ResetPassword, SetUserRoles,
        VerifyEmail,
    },
    error::CommandError,
    ids::UserId,
    queries::user::User,
    roles::Role,
};

async fn register(db: &dyn Db, email: &str, hash: &str) -> (UserId, Uuid) {
    let token = Uuid::new_v4();
    let id = db
        .register(Register {
            name: "Max Müller".into(),
            email: email.into(),
            password_hash: hash.into(),
            email_token: token,
        })
        .await
        .unwrap();
    (id, token)
}

async fn users(db: &dyn Db) -> Vec<User> {
    db.list_users(&admin()).await.unwrap()
}

async fn user(db: &dyn Db, id: UserId) -> User {
    users(db)
        .await
        .into_iter()
        .find(|user| user.id == id)
        .expect("user is listed")
}

async fn hash_of(db: &dyn Db, email: &str) -> String {
    db.login(Login {
        email: email.into(),
    })
    .await
    .unwrap()
    .password_hash
}

/// A new user has an unverified email; the email is unique; login hands out
/// the stored hash; a token verifies once.
pub async fn register_login_and_verify_email(db: &dyn Db) {
    let (id, token) = register(db, "max@example.org", "hash-1").await;
    let created = user(db, id).await;
    assert_eq!(created.name, "Max Müller");
    assert_eq!(created.email, "max@example.org");
    assert!(!created.email_verified);
    assert!(created.roles.is_empty());

    // The same email is refused, nothing is created.
    let before = users(db).await.len();
    let again = db
        .register(Register {
            name: "Erika Musterfrau".into(),
            email: "max@example.org".into(),
            password_hash: "hash-2".into(),
            email_token: Uuid::new_v4(),
        })
        .await;
    assert!(matches!(again, Err(CommandError::Rejected(_))));
    assert_eq!(users(db).await.len(), before);

    let credentials = db
        .login(Login {
            email: "max@example.org".into(),
        })
        .await
        .unwrap();
    assert_eq!(credentials.id, id);
    assert_eq!(credentials.password_hash, "hash-1");
    assert!(matches!(
        db.login(Login {
            email: "nobody@example.org".into()
        })
        .await,
        Err(CommandError::NotFound)
    ));

    // An unknown token verifies nobody; the right one does, once.
    assert!(matches!(
        db.verify_email(VerifyEmail {
            token: Uuid::new_v4()
        })
        .await,
        Err(CommandError::NotFound)
    ));
    assert!(!user(db, id).await.email_verified);
    assert_eq!(db.verify_email(VerifyEmail { token }).await.unwrap(), id);
    assert!(user(db, id).await.email_verified);
    assert!(matches!(
        db.verify_email(VerifyEmail { token }).await,
        Err(CommandError::NotFound)
    ));
}

/// Reset and verification tokens replace the open ones; a reset sets the
/// password and uses the token up.
pub async fn password_reset_and_mail_validation_tokens(db: &dyn Db) {
    let (id, first_token) = register(db, "erika@example.org", "old-hash").await;
    let actor = Actor::new(id, "Max Müller", Vec::new(), None);

    // Resending replaces the verification token.
    let second_token = Uuid::new_v4();
    db.resend_mail_validation(
        &actor,
        ResendMailValidation {
            token: second_token,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        db.verify_email(VerifyEmail { token: first_token }).await,
        Err(CommandError::NotFound)
    ));
    assert_eq!(
        db.verify_email(VerifyEmail {
            token: second_token
        })
        .await
        .unwrap(),
        id
    );

    // A reset for an unknown email changes nothing.
    assert!(matches!(
        db.request_password_reset(RequestPasswordReset {
            email: "nobody@example.org".into(),
            token: Uuid::new_v4(),
        })
        .await,
        Err(CommandError::NotFound)
    ));

    let first_reset = Uuid::new_v4();
    let contact = db
        .request_password_reset(RequestPasswordReset {
            email: "erika@example.org".into(),
            token: first_reset,
        })
        .await
        .unwrap();
    assert_eq!(contact.id, id);
    assert_eq!(contact.name, "Max Müller");
    // A second request drops the first token.
    let second_reset = Uuid::new_v4();
    db.request_password_reset(RequestPasswordReset {
        email: "erika@example.org".into(),
        token: second_reset,
    })
    .await
    .unwrap();
    assert!(matches!(
        db.reset_password(ResetPassword {
            token: first_reset,
            password_hash: "stolen".into(),
        })
        .await,
        Err(CommandError::NotFound)
    ));
    assert_eq!(hash_of(db, "erika@example.org").await, "old-hash");

    db.reset_password(ResetPassword {
        token: second_reset,
        password_hash: "new-hash".into(),
    })
    .await
    .unwrap();
    assert_eq!(hash_of(db, "erika@example.org").await, "new-hash");
    assert!(matches!(
        db.reset_password(ResetPassword {
            token: second_reset,
            password_hash: "again".into(),
        })
        .await,
        Err(CommandError::NotFound)
    ));
}

/// Roles replace each other and are listed sorted; nobody drops their own
/// Admin role; unknown users are not found.
pub async fn user_roles_are_set_and_listed(db: &dyn Db) {
    let world = World::seed(db).await;
    let boss = &world.organizer;
    let target = world.owner_a.user_id;

    db.set_user_roles(
        boss,
        SetUserRoles {
            user_id: target,
            roles: vec![Role::Treasurer, Role::JuryLead],
        },
    )
    .await
    .unwrap();
    assert_eq!(
        user(db, target).await.roles,
        vec![Role::JuryLead, Role::Treasurer]
    );
    // Exactly these roles afterwards.
    db.set_user_roles(
        boss,
        SetUserRoles {
            user_id: target,
            roles: vec![Role::CheckIn],
        },
    )
    .await
    .unwrap();
    assert_eq!(user(db, target).await.roles, vec![Role::CheckIn]);
    assert_eq!(user(db, target).await.club_id, Some(world.club_a));

    // The boss holds Admin through the actor only if the stored roles say so;
    // grant it, then try to drop it.
    db.set_user_roles(
        boss,
        SetUserRoles {
            user_id: boss.user_id,
            roles: vec![Role::Admin, Role::Treasurer],
        },
    )
    .await
    .unwrap();
    let dropped = db
        .set_user_roles(
            boss,
            SetUserRoles {
                user_id: boss.user_id,
                roles: vec![Role::Treasurer],
            },
        )
        .await;
    assert!(matches!(dropped, Err(CommandError::Forbidden)));
    assert_eq!(
        user(db, boss.user_id).await.roles,
        vec![Role::Admin, Role::Treasurer]
    );

    let missing = db
        .set_user_roles(
            boss,
            SetUserRoles {
                user_id: UserId::new(),
                roles: vec![Role::Admin],
            },
        )
        .await;
    assert!(matches!(missing, Err(CommandError::NotFound)));
    assert_eq!(users(db).await.len(), 5);
}
