//! The competition's settings and images through the trait: editing the
//! settings, the system status, the logo, break image and sponsors. Decoding
//! and downloading images stay with the server; a command stores the result.

use time::macros::datetime;

use super::{admin, timeplan::create_competition_with};
use crate::{
    Db,
    commands::{
        competition::EditCompetition,
        media::{
            AddSponsor, ImageInput, RemoveSponsor, ReorderSponsors, SetBreakImage,
            SetCompetitionLogo, SetImageSource, StoreFetchedImage, UpdateSponsor,
            UploadSponsorImage,
        },
        timeplan::SetStartlistPublished,
    },
    competition::{Competition, Features, JuryRules, PhotosSettings},
    error::CommandError,
    media::{Format, Image, Logo, Slot},
};

fn svg(body: &str) -> Logo {
    Logo {
        format: Format::Svg,
        data: format!("<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- {body} --></svg>")
            .into_bytes(),
    }
}

async fn images(db: &dyn Db) -> Vec<Image> {
    db.get_competition_images(&admin()).await.unwrap()
}

fn image_of<'a>(images: &'a [Image], kind: &str) -> Option<&'a Image> {
    images.iter().find(|image| image.kind == kind)
}

fn sponsor_names(images: &[Image]) -> Vec<String> {
    images
        .iter()
        .filter(|image| image.kind == "sponsor")
        .map(|image| image.name.clone())
        .collect()
}

async fn add(db: &dyn Db, name: &str, image: Option<ImageInput>) -> i64 {
    db.add_sponsor(
        &admin(),
        AddSponsor {
            name: name.into(),
            link: None,
            image,
        },
    )
    .await
    .unwrap()
}

/// Reading and editing the settings, and the system status.
pub async fn competition_settings_edit_and_status(db: &dyn Db) {
    let admin = admin();
    // Before any competition.
    let status = db.get_system_status().await.unwrap();
    assert_eq!(status.competition, None);
    assert!(!status.startlist_published);
    assert!(matches!(
        db.get_competition_settings(&admin).await,
        Err(CommandError::NotFound)
    ));

    create_competition_with(db, |_| {}).await;
    let current = db.get_competition_settings(&admin).await.unwrap();
    assert_eq!(current.name, "Konformität 2027");
    assert_eq!(current.primary_color, None);

    // The id of the request is ignored, the current competition is edited;
    // both colors are set (`None` resets them), photos keep their password.
    let edited = Competition {
        id: "another-id".into(),
        name: "Neuer Name".into(),
        venue: Some("Halle 1".into()),
        jury: Some(JuryRules {
            min_age: 16,
            deployments_per_two_acts: 4,
            missing_deployment_fee: 25.0,
            registration_end: Some(datetime!(2027-04-01 12:00 UTC)),
        }),
        features: Features {
            jury_registration: Some(false),
            moderation_input: Some(true),
        },
        photos: Some(PhotosSettings {
            source: "https://cloud.example.org/s/abc".into(),
            user: Some("fotos".into()),
            password: Some("geheim".into()),
        }),
        primary_color: Some("#002d56".parse().unwrap()),
        secondary_color: None,
        ..current.clone()
    };
    db.edit_competition(
        &admin,
        EditCompetition {
            competition: edited.clone(),
        },
    )
    .await
    .unwrap();
    let after = db.get_competition_settings(&admin).await.unwrap();
    assert_eq!(
        after,
        Competition {
            id: current.id.clone(),
            ..edited.clone()
        }
    );

    // Editing again with no colors resets the color that was set.
    db.edit_competition(
        &admin,
        EditCompetition {
            competition: Competition {
                primary_color: None,
                ..edited
            },
        },
    )
    .await
    .unwrap();
    assert_eq!(
        db.get_competition_settings(&admin)
            .await
            .unwrap()
            .primary_color,
        None
    );

    // The status carries the competition and the published flag, as one.
    let status = db.get_system_status().await.unwrap();
    assert_eq!(status.competition.unwrap().name, "Neuer Name");
    assert!(!status.startlist_published);
    db.set_startlist_published(
        &admin,
        SetStartlistPublished {
            published: true,
            at: datetime!(2027-04-30 10:00 UTC),
        },
    )
    .await
    .unwrap();
    assert!(db.get_system_status().await.unwrap().startlist_published);
}

/// Without a competition nothing can be stored; a missing sponsor is not found.
pub async fn media_commands_need_a_competition_and_a_sponsor(db: &dyn Db) {
    let admin = admin();
    assert!(matches!(
        db.edit_competition(
            &admin,
            EditCompetition {
                competition: super::timeplan::competition(),
            },
        )
        .await,
        Err(CommandError::NotFound)
    ));
    assert!(matches!(
        db.set_competition_logo(&admin, SetCompetitionLogo { logo: svg("a") })
            .await,
        Err(CommandError::NotFound)
    ));
    assert!(matches!(
        db.add_sponsor(
            &admin,
            AddSponsor {
                name: "X".into(),
                link: None,
                image: None,
            },
        )
        .await,
        Err(CommandError::NotFound)
    ));
    assert!(images(db).await.is_empty());
    assert!(db.list_sponsors().await.unwrap().is_empty());

    create_competition_with(db, |_| {}).await;
    assert!(matches!(
        db.upload_sponsor_image(
            &admin,
            UploadSponsorImage {
                sponsor_id: 99,
                logo: svg("a"),
            },
        )
        .await,
        Err(CommandError::NotFound)
    ));
    assert!(matches!(
        db.update_sponsor(
            &admin,
            UpdateSponsor {
                id: 99,
                name: "X".into(),
                link: None,
            },
        )
        .await,
        Err(CommandError::NotFound)
    ));
    assert!(matches!(
        db.set_image_source(
            &admin,
            SetImageSource {
                slot: Slot::Sponsor(99),
                source: ImageInput::Url("https://example.org/a.svg".into()),
            },
        )
        .await,
        Err(CommandError::NotFound)
    ));
    // Removing what isn't there is fine.
    db.remove_sponsor(&admin, RemoveSponsor { id: 99 })
        .await
        .unwrap();
    db.remove_competition_logo(&admin).await.unwrap();
    db.remove_break_image(&admin).await.unwrap();
}

/// The logo, the break image and their removal.
pub async fn logo_and_break_image_are_stored_and_removed(db: &dyn Db) {
    let admin = admin();
    create_competition_with(db, |_| {}).await;
    assert!(images(db).await.is_empty());

    let logo = svg("logo");
    db.set_competition_logo(&admin, SetCompetitionLogo { logo: logo.clone() })
        .await
        .unwrap();
    db.set_break_image(&admin, SetBreakImage { logo: svg("break") })
        .await
        .unwrap();
    let all = images(db).await;
    let stored = image_of(&all, "logo").unwrap();
    assert!(stored.has_data);
    assert_eq!(stored.mime_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(stored.source_url, None);
    assert!(image_of(&all, "break").unwrap().has_data);

    // A URL source keeps the old bytes until the first good download.
    let url = "https://example.org/logo.svg";
    db.set_image_source(
        &admin,
        SetImageSource {
            slot: Slot::Logo,
            source: ImageInput::Url(url.into()),
        },
    )
    .await
    .unwrap();
    let all = images(db).await;
    let stored = image_of(&all, "logo").unwrap();
    assert_eq!(stored.source_url.as_deref(), Some(url));
    assert!(stored.has_data);

    // The download's result is stored; one for another URL is dropped.
    let id = stored.id;
    assert!(
        !db.store_fetched_image(StoreFetchedImage {
            id,
            url: "https://example.org/other.svg".into(),
            result: Ok(svg("late")),
        })
        .await
        .unwrap()
    );
    assert!(
        db.store_fetched_image(StoreFetchedImage {
            id,
            url: url.into(),
            result: Err("Der Server ist nicht erreichbar.".into()),
        })
        .await
        .unwrap()
    );
    let all = images(db).await;
    let stored = image_of(&all, "logo").unwrap();
    assert_eq!(
        stored.fetch_error.as_deref(),
        Some("Der Server ist nicht erreichbar.")
    );
    assert!(stored.has_data, "a failed download keeps the last copy");
    assert!(
        db.store_fetched_image(StoreFetchedImage {
            id,
            url: url.into(),
            result: Ok(svg("downloaded")),
        })
        .await
        .unwrap()
    );
    let all = images(db).await;
    assert_eq!(image_of(&all, "logo").unwrap().fetch_error, None);

    // An inline SVG replaces the URL source.
    db.set_image_source(
        &admin,
        SetImageSource {
            slot: Slot::Logo,
            source: ImageInput::Data(logo),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        image_of(&images(db).await, "logo").unwrap().source_url,
        None
    );

    db.remove_competition_logo(&admin).await.unwrap();
    db.remove_break_image(&admin).await.unwrap();
    assert!(images(db).await.is_empty());
}

/// Sponsors: listed in order, only with an image; reorder, change, remove.
pub async fn sponsors_are_added_ordered_and_listed(db: &dyn Db) {
    let admin = admin();
    create_competition_with(db, |_| {}).await;

    let a = add(db, "Alpha", None).await;
    let b = add(db, "Beta", Some(ImageInput::Data(svg("b")))).await;
    let c = add(
        db,
        "Gamma",
        Some(ImageInput::Url("https://example.org/c.svg".into())),
    )
    .await;
    assert_eq!(sponsor_names(&images(db).await), ["Alpha", "Beta", "Gamma"]);
    // The public list has only those with bytes: a URL that was never
    // downloaded and a sponsor without an image are left out.
    let listed = db.list_sponsors().await.unwrap();
    assert_eq!(listed.iter().map(|s| s.id).collect::<Vec<_>>(), vec![b]);

    db.upload_sponsor_image(
        &admin,
        UploadSponsorImage {
            sponsor_id: a,
            logo: svg("a"),
        },
    )
    .await
    .unwrap();
    db.update_sponsor(
        &admin,
        UpdateSponsor {
            id: a,
            name: "Alpha AG".into(),
            link: Some("https://example.org/alpha".into()),
        },
    )
    .await
    .unwrap();
    let listed = db.list_sponsors().await.unwrap();
    assert_eq!(listed.iter().map(|s| s.id).collect::<Vec<_>>(), vec![a, b]);
    assert_eq!(listed[0].name, "Alpha AG");
    assert_eq!(listed[0].link.as_deref(), Some("https://example.org/alpha"));

    // The order must name every sponsor once; a refusal changes nothing.
    for ids in [vec![c, b], vec![c, b, b], vec![c, b, a, 99]] {
        let refused = db.reorder_sponsors(&admin, ReorderSponsors { ids }).await;
        assert!(
            matches!(refused, Err(CommandError::Rejected(_))),
            "{refused:?}"
        );
    }
    assert_eq!(
        sponsor_names(&images(db).await),
        ["Alpha AG", "Beta", "Gamma"]
    );
    db.reorder_sponsors(&admin, ReorderSponsors { ids: vec![c, b, a] })
        .await
        .unwrap();
    assert_eq!(
        sponsor_names(&images(db).await),
        ["Gamma", "Beta", "Alpha AG"]
    );

    db.remove_sponsor(&admin, RemoveSponsor { id: b })
        .await
        .unwrap();
    assert_eq!(sponsor_names(&images(db).await), ["Gamma", "Alpha AG"]);
    assert_eq!(db.list_sponsors().await.unwrap().len(), 1);
}
