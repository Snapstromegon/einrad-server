//! The demo data: what it writes into an empty database.

use std::path::PathBuf;

use db_sqlite::SqliteRepo;
use einrad_server::demo::DemoData;
use uuid::Uuid;

const DEMO: &str = include_str!("../demo-data.toml");

async fn count(db: &sqlx::SqlitePool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(db).await.unwrap()
}

fn data_dir() -> PathBuf {
    std::env::temp_dir().join(format!("cup-demo-test-{}", Uuid::new_v4()))
}

#[tokio::test]
async fn demo_file_fills_every_table() {
    let db = db_sqlite::reloadable_sqlite::in_memory_migrated().await;
    let data = DemoData::parse(DEMO).unwrap();
    let data_path = data_dir();
    data.apply(&SqliteRepo::from_pool(db.clone()), &data_path)
        .await
        .unwrap();

    for (table, sql) in [
        ("competition", "SELECT COUNT(*) FROM competition"),
        ("users", "SELECT COUNT(*) FROM users"),
        (
            "mail_verification",
            "SELECT COUNT(*) FROM mail_verification",
        ),
        ("password_resets", "SELECT COUNT(*) FROM password_resets"),
        ("clubs", "SELECT COUNT(*) FROM clubs"),
        ("rider", "SELECT COUNT(*) FROM rider"),
        ("acts", "SELECT COUNT(*) FROM acts"),
        ("act_participants", "SELECT COUNT(*) FROM act_participants"),
        ("categories", "SELECT COUNT(*) FROM categories"),
        ("timeplan", "SELECT COUNT(*) FROM timeplan"),
        ("finals", "SELECT COUNT(*) FROM finals"),
        ("final_slots", "SELECT COUNT(*) FROM final_slots"),
        (
            "planned finals",
            "SELECT COUNT(*) FROM timeplan WHERE final_id IS NOT NULL",
        ),
    ] {
        assert!(count(&db, sql).await > 0, "{table} is empty");
    }

    // Every act lands in a category, so it shows up in the startlist,
    // except for one group that doesn't have enough riders yet.
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM view_act WHERE category IS NULL AND NOT (group_name IS NOT NULL AND regular_count < 3)"
        )
        .await,
        0
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM view_act WHERE category IS NULL").await,
        1
    );
    for (what, sql) in [
        (
            "small group",
            "SELECT COUNT(*) FROM view_act WHERE act_type = 'group_small' AND category IS NOT NULL",
        ),
        (
            "large group",
            "SELECT COUNT(*) FROM view_act WHERE act_type = 'group_large' AND category IS NOT NULL",
        ),
        (
            "group across clubs",
            "SELECT COUNT(*) FROM view_act WHERE group_name IS NOT NULL AND (SELECT COUNT(DISTINCT s.club_id) FROM act_participants p JOIN rider s ON s.id = p.rider_id WHERE p.act_id = view_act.id) > 1",
        ),
        (
            "replacement rider",
            "SELECT COUNT(*) FROM act_participants WHERE is_replacement",
        ),
        (
            "checked-in rider",
            "SELECT COUNT(*) FROM rider WHERE checked_in_at IS NOT NULL",
        ),
        (
            "rider not checked in",
            "SELECT COUNT(*) FROM rider WHERE checked_in_at IS NULL",
        ),
    ] {
        assert!(count(&db, sql).await > 0, "no {what}");
    }
    // Pairs have two riders.
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM acts WHERE act_type = 'pair' AND (SELECT COUNT(*) FROM act_participants p WHERE p.act_id = acts.id) != 2"
        )
        .await,
        0
    );
    for (what, sql) in [
        (
            "linked pair",
            "SELECT COUNT(*) FROM acts WHERE act_type = 'pair'",
        ),
        ("pair request", "SELECT COUNT(*) FROM pair_requests"),
    ] {
        assert!(count(&db, sql).await > 0, "no {what}");
    }
    let songs: Vec<String> =
        sqlx::query_scalar("SELECT song_file FROM acts WHERE song_file IS NOT NULL")
            .fetch_all(&db)
            .await
            .unwrap();
    assert!(!songs.is_empty());
    for song in songs {
        assert!(data_path.join(&song).exists(), "{song}");
    }
    std::fs::remove_dir_all(data_path).unwrap();
}

#[tokio::test]
async fn demo_structure_has_no_loose_ends() {
    let db = db_sqlite::reloadable_sqlite::in_memory_migrated().await;
    let data = DemoData::parse(DEMO).unwrap();
    let data_path = data_dir();
    data.apply(&SqliteRepo::from_pool(db.clone()), &data_path)
        .await
        .unwrap();

    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM categories WHERE heat_id IS NULL").await,
        0,
        "a category has no heat"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM heats WHERE NOT EXISTS (SELECT 1 FROM final_slots WHERE final_slots.heat_id = heats.id)"
        )
        .await,
        0,
        "a heat feeds no final"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM finals WHERE NOT EXISTS (SELECT 1 FROM timeplan WHERE timeplan.final_id = finals.id)"
        )
        .await,
        0,
        "a final is not in the timeplan"
    );
    std::fs::remove_dir_all(data_path).unwrap();
}

#[tokio::test]
async fn unknown_partner_is_rejected() {
    let db = db_sqlite::reloadable_sqlite::in_memory_migrated().await;
    let mut data = DemoData::parse(DEMO).unwrap();
    let rider = &mut data.clubs[0].riders[0];
    rider.pair = true;
    rider.partner = Some("Niemand Unbekannt".into());
    rider.partner_name = None;
    let err = data
        .apply(&SqliteRepo::from_pool(db.clone()), &data_dir())
        .await
        .unwrap_err();
    assert!(err.contains("Niemand Unbekannt"), "{err}");
}

#[tokio::test]
async fn demo_groups_follow_the_group_rules() {
    let db = db_sqlite::reloadable_sqlite::in_memory_migrated().await;
    let mut data = DemoData::parse(DEMO).unwrap();
    // Emma Wagner already rides in a small group.
    let incomplete = data
        .groups
        .iter_mut()
        .find(|group| group.members.len() < 3)
        .unwrap();
    incomplete.members.push("Emma Wagner".into());
    let err = data
        .apply(&SqliteRepo::from_pool(db.clone()), &data_dir())
        .await
        .unwrap_err();
    assert!(err.contains("„Emma Wagner“"), "{err}");
}

#[tokio::test]
async fn fetched_songs_are_stored() {
    let db = db_sqlite::reloadable_sqlite::in_memory_migrated().await;
    let mut data = DemoData::parse(DEMO).unwrap();
    // No downloads in tests: one song is in the cache already, the
    // other's download fails.
    for act in &mut data.acts {
        act.song_url = None;
    }
    let cache = data_dir();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("cached.mp3"), b"cached song").unwrap();
    let (cached, missing) = {
        let mut with_song = data.acts.iter_mut().filter(|act| act.song.is_some());
        let cached = with_song.next().unwrap();
        cached.song_url = Some("https://example.org/songs/cached.mp3".into());
        let missing = with_song.next().unwrap();
        missing.song_url = Some(format!("file://{}", cache.join("not-there.mp3").display()));
        (cached.song.clone(), missing.song.clone())
    };

    data.fetch_songs(&cache).unwrap();
    let missing_act = data.acts.iter().find(|act| act.song == missing).unwrap();
    assert_eq!(missing_act.song_source, None);
    assert!(!cache.join("not-there.mp3").exists());
    assert!(!cache.join("not-there.part").exists());

    let data_path = data_dir();
    data.apply(&SqliteRepo::from_pool(db.clone()), &data_path)
        .await
        .unwrap();
    let file: String = sqlx::query_scalar("SELECT song_file FROM acts WHERE song_file_name = $1")
        .bind(&cached)
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(file.ends_with(".mp3"), "{file}");
    assert_eq!(std::fs::read(data_path.join(file)).unwrap(), b"cached song");
    std::fs::remove_dir_all(data_path).unwrap();
    std::fs::remove_dir_all(cache).unwrap();
}

#[test]
fn song_url_needs_a_file_name() {
    let mut data = DemoData::parse(DEMO).unwrap();
    for act in &mut data.acts {
        act.song_url = None;
    }
    data.acts[0].song_url = Some("https://example.org/".into());
    let cache = data_dir();
    let err = data.fetch_songs(&cache).unwrap_err();
    assert!(err.contains("https://example.org/"), "{err}");
    std::fs::remove_dir_all(cache).unwrap();
}
