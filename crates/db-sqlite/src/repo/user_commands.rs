//! `UserCommands` for SQLite.

use db_core::{
    actor::Actor,
    commands::user::{
        Credentials, Login, Register, RequestPasswordReset, ResendMailValidation, ResetPassword,
        SetUserRoles, UserCommands, UserContact, VerifyEmail,
    },
    error::CommandError,
    ids::UserId,
    roles::Role,
};
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{SqliteRepo, read_tx, write_tx};

#[async_trait::async_trait]
impl UserCommands for SqliteRepo {
    async fn register(&self, cmd: Register) -> Result<UserId, CommandError> {
        Ok(write_tx!(self, tx, { register_in(&mut tx, cmd).await? }))
    }

    async fn login(&self, cmd: Login) -> Result<Credentials, CommandError> {
        // Nothing is written: a read transaction.
        read_tx!(self, tx, {
            credentials_in(&mut tx, &cmd.email)
                .await?
                .ok_or(CommandError::NotFound)
        })
    }

    async fn verify_email(&self, cmd: VerifyEmail) -> Result<UserId, CommandError> {
        // An unknown token changes nothing, so dropping the transaction is
        // the rollback.
        Ok(write_tx!(self, tx, {
            verify_email_in(&mut tx, cmd.token).await?
        }))
    }

    async fn request_password_reset(
        &self,
        cmd: RequestPasswordReset,
    ) -> Result<UserContact, CommandError> {
        Ok(write_tx!(self, tx, {
            request_password_reset_in(&mut tx, cmd).await?
        }))
    }

    async fn reset_password(&self, cmd: ResetPassword) -> Result<(), CommandError> {
        write_tx!(self, tx, { reset_password_in(&mut tx, cmd).await? });
        Ok(())
    }

    async fn resend_mail_validation(
        &self,
        actor: &Actor,
        cmd: ResendMailValidation,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            replace_verification_token(&mut tx, actor.user_id, cmd.token).await?
        });
        Ok(())
    }

    async fn set_user_roles(&self, actor: &Actor, cmd: SetUserRoles) -> Result<(), CommandError> {
        cmd.check(actor)?;
        write_tx!(self, tx, { set_user_roles_in(&mut tx, cmd).await? });
        Ok(())
    }
}

async fn register_in(conn: &mut SqliteConnection, cmd: Register) -> Result<UserId, CommandError> {
    let taken = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM users WHERE email = $1"#,
        cmd.email
    )
    .fetch_one(&mut *conn)
    .await?;
    if taken > 0 {
        return Err(CommandError::Rejected(
            "Email Adresse bereits registriert.".to_string(),
        ));
    }
    let id = UserId::new();
    sqlx::query!(
        "INSERT INTO users (id, name, email, email_verified, password) VALUES ($1, $2, $3, false, $4)",
        id,
        cmd.name,
        cmd.email,
        cmd.password_hash,
    )
    .execute(&mut *conn)
    .await?;
    let now = time::OffsetDateTime::now_utc();
    sqlx::query!(
        "INSERT INTO mail_verification (id, user_id, created_at) VALUES ($1, $2, $3)",
        cmd.email_token,
        id,
        now
    )
    .execute(&mut *conn)
    .await?;
    Ok(id)
}

async fn credentials_in(
    conn: &mut SqliteConnection,
    email: &str,
) -> Result<Option<Credentials>, CommandError> {
    let row = sqlx::query!(
        r#"SELECT id as "id!: UserId", password FROM users WHERE email = $1"#,
        email,
    )
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row.map(|row| Credentials {
        id: row.id,
        password_hash: row.password,
    }))
}

async fn verify_email_in(conn: &mut SqliteConnection, token: Uuid) -> Result<UserId, CommandError> {
    let Some(row) = sqlx::query!(
        r#"SELECT user_id as "user_id!: UserId" FROM mail_verification WHERE id = $1"#,
        token
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Err(CommandError::NotFound);
    };
    sqlx::query!(
        "UPDATE users SET email_verified = true WHERE id = $1",
        row.user_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "DELETE FROM mail_verification WHERE user_id = $1",
        row.user_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(row.user_id)
}

async fn replace_verification_token(
    conn: &mut SqliteConnection,
    user_id: UserId,
    token: Uuid,
) -> Result<(), CommandError> {
    sqlx::query!("DELETE FROM mail_verification WHERE user_id = $1", user_id)
        .execute(&mut *conn)
        .await?;
    let now = time::OffsetDateTime::now_utc();
    sqlx::query!(
        "INSERT INTO mail_verification (id, user_id, created_at) VALUES ($1, $2, $3)",
        token,
        user_id,
        now
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn request_password_reset_in(
    conn: &mut SqliteConnection,
    cmd: RequestPasswordReset,
) -> Result<UserContact, CommandError> {
    let Some(row) = sqlx::query!(
        r#"SELECT id as "id!: UserId", name FROM users WHERE email = $1"#,
        cmd.email
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Err(CommandError::NotFound);
    };
    sqlx::query!("DELETE FROM password_resets WHERE user_id = $1", row.id)
        .execute(&mut *conn)
        .await?;
    let now = time::OffsetDateTime::now_utc();
    sqlx::query!(
        "INSERT INTO password_resets (id, user_id, created_at) VALUES ($1, $2, $3)",
        cmd.token,
        row.id,
        now
    )
    .execute(&mut *conn)
    .await?;
    Ok(UserContact {
        id: row.id,
        name: row.name,
    })
}

async fn reset_password_in(
    conn: &mut SqliteConnection,
    cmd: ResetPassword,
) -> Result<(), CommandError> {
    let Some(user_id) = sqlx::query_scalar!(
        r#"SELECT user_id as "user_id!: UserId" FROM password_resets WHERE id = $1"#,
        cmd.token
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Err(CommandError::NotFound);
    };
    sqlx::query!(
        "UPDATE users SET password = $1 WHERE id = $2",
        cmd.password_hash,
        user_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!("DELETE FROM password_resets WHERE user_id = $1", user_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn set_user_roles_in(
    conn: &mut SqliteConnection,
    cmd: SetUserRoles,
) -> Result<(), CommandError> {
    let exists = sqlx::query_scalar!("SELECT 1 FROM users WHERE id = $1", cmd.user_id)
        .fetch_optional(&mut *conn)
        .await?;
    if exists.is_none() {
        return Err(CommandError::NotFound);
    }
    sqlx::query!("DELETE FROM user_roles WHERE user_id = $1", cmd.user_id)
        .execute(&mut *conn)
        .await?;
    for role in &cmd.roles {
        let role: Role = *role;
        sqlx::query!(
            "INSERT INTO user_roles (user_id, role) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            cmd.user_id,
            role
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}
