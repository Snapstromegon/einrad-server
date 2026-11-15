//! `MediaCommands` for SQLite.

use db_core::{
    actor::Actor,
    commands::media::{
        AddSponsor, ImageInput, MediaCommands, RemoveSponsor, ReorderSponsors, SetBreakImage,
        SetCompetitionLogo, SetImageSource, StoreFetchedImage, UpdateSponsor, UploadSponsorImage,
    },
    error::CommandError,
    media::{Logo, Slot},
};
use sqlx::SqliteConnection;

use super::{SqliteRepo, write_tx};
use crate::{competition, images};

#[async_trait::async_trait]
impl MediaCommands for SqliteRepo {
    async fn set_competition_logo(
        &self,
        _actor: &Actor,
        cmd: SetCompetitionLogo,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_data(&mut tx, Slot::Logo, &cmd.logo).await?
        });
        Ok(())
    }

    async fn remove_competition_logo(&self, _actor: &Actor) -> Result<(), CommandError> {
        write_tx!(self, tx, { images::remove(&mut *tx, Slot::Logo).await? });
        Ok(())
    }

    async fn set_break_image(
        &self,
        _actor: &Actor,
        cmd: SetBreakImage,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_data(&mut tx, Slot::Break, &cmd.logo).await?
        });
        Ok(())
    }

    async fn remove_break_image(&self, _actor: &Actor) -> Result<(), CommandError> {
        write_tx!(self, tx, { images::remove(&mut *tx, Slot::Break).await? });
        Ok(())
    }

    async fn upload_sponsor_image(
        &self,
        _actor: &Actor,
        cmd: UploadSponsorImage,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_data(&mut tx, Slot::Sponsor(cmd.sponsor_id), &cmd.logo).await?
        });
        Ok(())
    }

    async fn add_sponsor(&self, _actor: &Actor, cmd: AddSponsor) -> Result<i64, CommandError> {
        Ok(write_tx!(self, tx, { add_sponsor(&mut tx, cmd).await? }))
    }

    async fn update_sponsor(&self, _actor: &Actor, cmd: UpdateSponsor) -> Result<(), CommandError> {
        write_tx!(self, tx, { update_sponsor(&mut tx, cmd).await? });
        Ok(())
    }

    async fn remove_sponsor(&self, _actor: &Actor, cmd: RemoveSponsor) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            images::remove(&mut *tx, Slot::Sponsor(cmd.id)).await?
        });
        Ok(())
    }

    async fn reorder_sponsors(
        &self,
        _actor: &Actor,
        cmd: ReorderSponsors,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, { reorder_sponsors(&mut tx, &cmd.ids).await? });
        Ok(())
    }

    async fn set_image_source(
        &self,
        _actor: &Actor,
        cmd: SetImageSource,
    ) -> Result<(), CommandError> {
        write_tx!(self, tx, {
            set_input(&mut tx, cmd.slot, &cmd.source).await?
        });
        Ok(())
    }

    async fn store_fetched_image(&self, cmd: StoreFetchedImage) -> Result<bool, CommandError> {
        Ok(write_tx!(self, tx, {
            images::store_fetched(&mut *tx, cmd.id, &cmd.url, cmd.result).await?
        }))
    }
}

/// Stores `logo` in `slot` of the current competition; `NotFound` without a
/// competition or such a sponsor.
async fn set_data(
    conn: &mut SqliteConnection,
    slot: Slot,
    logo: &Logo,
) -> Result<(), CommandError> {
    let Some(competition_id) = competition::current_id(&mut *conn).await? else {
        return Err(CommandError::NotFound);
    };
    if !images::set_data(&mut *conn, &competition_id, slot, logo).await? {
        return Err(CommandError::NotFound);
    }
    Ok(())
}

/// Sets the image in `slot` from data or a URL (nothing is downloaded here).
async fn set_input(
    conn: &mut SqliteConnection,
    slot: Slot,
    input: &ImageInput,
) -> Result<(), CommandError> {
    match input {
        ImageInput::Data(logo) => set_data(conn, slot, logo).await,
        ImageInput::Url(url) => {
            let Some(competition_id) = competition::current_id(&mut *conn).await? else {
                return Err(CommandError::NotFound);
            };
            if !images::set_url(&mut *conn, &competition_id, slot, url).await? {
                return Err(CommandError::NotFound);
            }
            Ok(())
        }
    }
}

/// Adds the sponsor and its image in one transaction.
async fn add_sponsor(conn: &mut SqliteConnection, cmd: AddSponsor) -> Result<i64, CommandError> {
    let Some(competition_id) = competition::current_id(&mut *conn).await? else {
        return Err(CommandError::NotFound);
    };
    let id =
        images::add_sponsor(&mut *conn, &competition_id, &cmd.name, cmd.link.as_deref()).await?;
    if let Some(image) = &cmd.image {
        set_input(conn, Slot::Sponsor(id), image).await?;
    }
    Ok(id)
}

async fn update_sponsor(
    conn: &mut SqliteConnection,
    cmd: UpdateSponsor,
) -> Result<(), CommandError> {
    if !images::update_sponsor(&mut *conn, cmd.id, &cmd.name, cmd.link.as_deref()).await? {
        return Err(CommandError::NotFound);
    }
    Ok(())
}

/// Puts the sponsors in the order of `ids`, if it names every sponsor
/// exactly once.
async fn reorder_sponsors(conn: &mut SqliteConnection, ids: &[i64]) -> Result<(), CommandError> {
    let mut current: Vec<i64> = images::list(&mut *conn)
        .await?
        .into_iter()
        .filter(|image| image.kind == "sponsor")
        .map(|image| image.id)
        .collect();
    let mut wanted = ids.to_vec();
    current.sort_unstable();
    wanted.sort_unstable();
    if current != wanted {
        return Err(CommandError::Rejected(
            "Die Reihenfolge muss alle Sponsoren genau einmal enthalten.".to_string(),
        ));
    }
    for (position, id) in (0..).zip(ids) {
        images::set_position(&mut *conn, *id, position).await?;
    }
    Ok(())
}
