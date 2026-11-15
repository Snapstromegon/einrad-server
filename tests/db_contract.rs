//! Guards of the API <-> DB contract (#238): the DB contract mirrors the API
//! (CQRS), one method per command or query endpoint, in exactly one area
//! trait under `db_core::commands` / `db_core::queries`.
//!
//! * The full OpenAPI snapshot (`tests/openapi_snapshot.json`: paths, parameters,
//!   request/response schemas, every component schema with fields, types,
//!   required, enums, formats and docs) proves the generated client stays unchanged.
//! * The mapping test compares the endpoints with the area traits' methods.
//!   `NOT_YET_MIGRATED` is a ratchet: it starts with every DB endpoint and
//!   may only shrink; a migrated endpoint that is still listed fails.
//! * The conformance coverage test wants every method exercised in
//!   `crates/db-core/src/conformance/`; `NOT_YET_COVERED` is a ratchet too.
//!
//! Regenerate the snapshot with `UPDATE_OPENAPI_SNAPSHOT=1 cargo test --test db_contract`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use einrad_server::http_server::routes::get_api_openapi;

/// Endpoints without a DB method of their own, with the reason.
const NON_DB: &[(&str, &str)] = &[
    ("logout", "cookie only"),
    ("reload_db", "Db capability (`Db::reload`)"),
    ("create_db_backup", "Db capability (`Db::backups`)"),
    ("delete_db_backups", "Db capability (`Db::backups`)"),
    ("restore_db_backup", "Db capability (`Db::backups`)"),
    ("list_db_backups", "Db capability (`Db::backups`)"),
    ("check_db_backup", "Db capability (`Db::backups`)"),
    ("get_playback", "player hub, no DB"),
    ("get_photos", "photos service, no DB"),
    ("set_photo_hidden", "photos service, no DB"),
    ("whoami", "session user comes through `Support`"),
];

/// Side-effect endpoints whose DB part has a different name:
/// (endpoint, the DB methods that stand for it). Empty until a step needs it.
const SPLIT: &[(&str, &[&str])] = &[];

/// DB endpoints (commands) whose area-trait method does not exist yet.
const NOT_YET_MIGRATED_COMMANDS: &[&str] = &[
    "add_category",
    "add_club_judge",
    "add_final",
    "add_heat",
    "add_sponsor",
    "add_timeplan_entry",
    "apply_jury_remap",
    "assign_judge",
    "clear_act_song",
    "create_competition",
    "delete_category",
    "delete_club_judge",
    "delete_final",
    "delete_heat",
    "delete_timeplan_entry",
    "edit_category",
    "edit_club_judge",
    "edit_competition",
    "edit_final",
    "edit_heat",
    "edit_jury_unit",
    "edit_timeplan_entry",
    "import_competition",
    "login",
    "move_category_down",
    "move_category_up",
    "move_judge_registrations",
    "move_timeplan_down",
    "move_timeplan_up",
    "preview_club_mail",
    "preview_competition_import",
    "refresh_images",
    "register",
    "remove_break_image",
    "remove_competition_logo",
    "remove_sponsor",
    "reorder_sponsors",
    "request_password_reset",
    "resend_mail_validation",
    "reset_block_order",
    "reset_password",
    "resolve_unmapped_registration",
    "save_act_song",
    "send_club_mail",
    "set_act_music_started",
    "set_act_order",
    "set_block_order",
    "set_break_image",
    "set_competition_logo",
    "set_final_placement",
    "set_final_published",
    "set_heat_skip",
    "set_image_source",
    "set_jury_deployments_override",
    "set_jury_plan_published",
    "set_song_checked",
    "set_startlist_published",
    "set_user_roles",
    "timeplan_backward",
    "timeplan_forward",
    "unassign_judge",
    "update_sponsor",
    "upload_sponsor_image",
    "verify_email",
];

/// DB endpoints (queries) whose area-trait method does not exist yet.
const NOT_YET_MIGRATED_QUERIES: &[&str] = &[
    "export_competition",
    "get_act",
    "get_club_mail_options",
    "get_competition_images",
    "get_competition_settings",
    "get_startlist_csv",
    "get_structure",
    "get_system_status",
    "list_acts",
    "list_categories",
    "list_club_judges",
    "list_club_mails",
    "list_finals",
    "list_heats",
    "list_judges",
    "list_jury_plan",
    "list_jury_units",
    "list_missing_songs",
    "list_sponsors",
    "list_timeplan",
    "list_users",
    "predict_timeplan",
    "startlist",
];

/// Methods that no conformance test calls yet (`.<name>(`).
/// Starts with every DB endpoint; shrinks as conformance tests land.
const NOT_YET_COVERED: &[&str] = &[
    "add_category",
    "add_club_judge",
    "add_final",
    "add_heat",
    "add_sponsor",
    "add_timeplan_entry",
    "apply_jury_remap",
    "assign_judge",
    "clear_act_song",
    "create_competition",
    "delete_category",
    "delete_club_judge",
    "delete_final",
    "delete_heat",
    "delete_timeplan_entry",
    "edit_category",
    "edit_club_judge",
    "edit_competition",
    "edit_final",
    "edit_heat",
    "edit_jury_unit",
    "edit_timeplan_entry",
    "import_competition",
    "login",
    "move_category_down",
    "move_category_up",
    "move_judge_registrations",
    "move_timeplan_down",
    "move_timeplan_up",
    "preview_club_mail",
    "preview_competition_import",
    "refresh_images",
    "register",
    "remove_break_image",
    "remove_competition_logo",
    "remove_sponsor",
    "reorder_sponsors",
    "request_password_reset",
    "resend_mail_validation",
    "reset_block_order",
    "reset_password",
    "resolve_unmapped_registration",
    "save_act_song",
    "send_club_mail",
    "set_act_music_started",
    "set_act_order",
    "set_block_order",
    "set_break_image",
    "set_competition_logo",
    "set_final_placement",
    "set_final_published",
    "set_heat_skip",
    "set_image_source",
    "set_jury_deployments_override",
    "set_jury_plan_published",
    "set_song_checked",
    "set_startlist_published",
    "set_user_roles",
    "timeplan_backward",
    "timeplan_forward",
    "unassign_judge",
    "update_sponsor",
    "upload_sponsor_image",
    "verify_email",
    "export_competition",
    "get_act",
    "get_club_mail_options",
    "get_competition_images",
    "get_competition_settings",
    "get_startlist_csv",
    "get_structure",
    "get_system_status",
    "list_acts",
    "list_categories",
    "list_club_judges",
    "list_club_mails",
    "list_finals",
    "list_heats",
    "list_judges",
    "list_jury_plan",
    "list_jury_units",
    "list_missing_songs",
    "list_sponsors",
    "list_timeplan",
    "list_users",
    "predict_timeplan",
    "startlist",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ── API snapshot ──

/// The whole OpenAPI document as normalized JSON: object keys sorted
/// (serde_json's `Value` keeps them in a `BTreeMap`), arrays in the order
/// utoipa emits them (deterministic: struct field order, path order), and
/// only `info.version` stripped, since it follows the crate version.
/// Doc descriptions stay in: they are part of the published API docs and the
/// generated client, and regenerating is one env var.
fn normalized_openapi() -> String {
    let mut value = serde_json::to_value(get_api_openapi()).expect("openapi serializes");
    if let Some(info) = value.get_mut("info").and_then(|i| i.as_object_mut()) {
        info.remove("version");
    }
    serde_json::to_string_pretty(&value).unwrap() + "\n"
}

/// Shows the first differing lines, so a changed field is obvious.
fn line_diff(expected: &str, current: &str) -> String {
    let e: Vec<&str> = expected.lines().collect();
    let c: Vec<&str> = current.lines().collect();
    let mut out = String::new();
    let mut shown = 0;
    let (mut i, mut j) = (0, 0);
    while (i < e.len() || j < c.len()) && shown < 40 {
        if i < e.len() && j < c.len() && e[i] == c[j] {
            i += 1;
            j += 1;
            continue;
        }
        // resync: look ahead for the next common line
        let sync = (1..200).find_map(|d| {
            (0..=d).find_map(|k| {
                let (a, b) = (i + k, j + d - k);
                (a < e.len() && b < c.len() && e[a] == c[b] && (a > i || b > j)).then_some((a, b))
            })
        });
        let (ni, nj) = sync.unwrap_or((e.len(), c.len()));
        for l in &e[i..ni] {
            out.push_str(&format!("- {l}\n"));
        }
        for l in &c[j..nj] {
            out.push_str(&format!("+ {l}\n"));
        }
        shown += (ni - i) + (nj - j);
        (i, j) = (ni, nj);
    }
    out
}

#[test]
fn the_openapi_document_matches_the_snapshot() {
    let snapshot_path = root().join("tests/openapi_snapshot.json");
    let current = normalized_openapi();
    if std::env::var_os("UPDATE_OPENAPI_SNAPSHOT").is_some() {
        std::fs::write(&snapshot_path, &current).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&snapshot_path)
        .expect("tests/openapi_snapshot.json is missing (UPDATE_OPENAPI_SNAPSHOT=1 creates it)");
    assert!(
        expected == current,
        "the OpenAPI document changed (- snapshot, + current); the generated client changes with it. \
         If intended, rerun with UPDATE_OPENAPI_SNAPSHOT=1 and review the diff of \
         tests/openapi_snapshot.json:\n{}",
        line_diff(&expected, &current)
    );
}

// ── endpoint <-> trait mapping ──

/// Names of the endpoints under `/api/<kind>/`.
fn endpoints(kind: &str) -> BTreeSet<String> {
    let prefix = format!("/api/{kind}/");
    get_api_openapi()
        .paths
        .paths
        .keys()
        .filter_map(|p| p.strip_prefix(&prefix))
        .map(str::to_string)
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every `async fn name` in the area trait files of `kind` ("commands" or
/// "queries"): `crates/db-core/src/<kind>/*.rs`, with its file.
fn trait_methods(kind: &str) -> Vec<(String, String)> {
    let mut files = Vec::new();
    rust_files(&root().join("crates/db-core/src").join(kind), &mut files);
    files.sort();
    let mut methods = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let label = file.file_name().unwrap().to_string_lossy().to_string();
        for line in text.lines() {
            let line = line.trim_start();
            if line.starts_with("//") {
                continue;
            }
            if let Some(rest) = line.strip_prefix("async fn ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                methods.push((name, label.clone()));
            }
        }
    }
    methods
}

fn check_mapping(kind: &str, trait_dir: &str, not_yet: &[&str]) {
    let all = endpoints(kind);
    let non_db: BTreeSet<&str> = NON_DB.iter().map(|(n, _)| *n).collect();
    let db_endpoints: BTreeSet<&str> = all
        .iter()
        .map(String::as_str)
        .filter(|n| !non_db.contains(n))
        .collect();
    let split: BTreeMap<&str, &[&str]> = SPLIT.iter().copied().collect();

    for (name, reason) in NON_DB {
        assert!(!reason.is_empty(), "NON_DB `{name}` needs a reason");
    }
    for name in not_yet {
        assert!(
            db_endpoints.contains(name),
            "NOT_YET_MIGRATED lists `{name}`, which is no {kind} DB endpoint"
        );
    }

    // The methods the traits must offer: every DB endpoint not yet migrated
    // and not split, plus the DB parts of split endpoints.
    let mut expected: BTreeSet<String> = BTreeSet::new();
    for name in &db_endpoints {
        if not_yet.contains(name) {
            continue;
        }
        match split.get(name) {
            Some(parts) => expected.extend(parts.iter().map(|p| p.to_string())),
            None => {
                expected.insert(name.to_string());
            }
        }
    }

    let methods = trait_methods(trait_dir);
    let mut seen: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (name, file) in &methods {
        seen.entry(name).or_default().push(file);
    }
    for (name, files) in &seen {
        assert!(
            files.len() == 1,
            "`{name}` must be in exactly one area trait, found in {files:?}"
        );
    }
    let actual: BTreeSet<String> = seen.keys().map(|n| n.to_string()).collect();

    let missing: Vec<_> = expected.difference(&actual).collect();
    let extra: Vec<_> = actual.difference(&expected).collect();
    assert!(
        missing.is_empty(),
        "{kind} endpoints without a method in {trait_dir}/: {missing:?}"
    );
    // A method that exists although the endpoint is still listed (or that has
    // no endpoint at all).
    let listed: Vec<_> = extra
        .iter()
        .filter(|n| not_yet.contains(&n.as_str()))
        .collect();
    assert!(
        listed.is_empty(),
        "migrated: remove from NOT_YET_MIGRATED: {listed:?}"
    );
    assert!(
        extra.is_empty(),
        "methods in {trait_dir}/ that no {kind} endpoint maps to (add to SPLIT or NON_DB?): {extra:?}"
    );
}

#[test]
fn every_command_endpoint_is_one_method_of_one_area_trait() {
    check_mapping("command", "commands", NOT_YET_MIGRATED_COMMANDS);
}

#[test]
fn every_query_endpoint_is_one_method_of_one_area_trait() {
    check_mapping("query", "queries", NOT_YET_MIGRATED_QUERIES);
}

#[test]
fn no_endpoint_is_both_non_db_and_not_yet_migrated() {
    let non_db: BTreeSet<&str> = NON_DB.iter().map(|(n, _)| *n).collect();
    for name in NOT_YET_MIGRATED_COMMANDS
        .iter()
        .chain(NOT_YET_MIGRATED_QUERIES)
    {
        assert!(!non_db.contains(name), "`{name}` is in both lists");
    }
    let all: BTreeSet<String> = endpoints("command")
        .into_iter()
        .chain(endpoints("query"))
        .collect();
    for (name, _) in NON_DB {
        assert!(all.contains(*name), "NON_DB `{name}` is no endpoint");
    }
    for (name, _) in SPLIT {
        assert!(all.contains(*name), "SPLIT `{name}` is no endpoint");
    }
}

// ── conformance coverage ──

#[test]
fn every_method_is_called_by_a_conformance_test() {
    let mut files = Vec::new();
    rust_files(&root().join("crates/db-core/src/conformance"), &mut files);
    let sources: String = files
        .iter()
        .map(|f| std::fs::read_to_string(f).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let methods: BTreeSet<String> = trait_methods("commands")
        .into_iter()
        .chain(trait_methods("queries"))
        .map(|(n, _)| n)
        .collect();
    let covered = |name: &str| sources.contains(&format!(".{name}("));

    let known: BTreeSet<String> = endpoints("command")
        .into_iter()
        .chain(endpoints("query"))
        .collect();
    for name in NOT_YET_COVERED {
        assert!(
            known.contains(*name) || methods.contains(*name),
            "NOT_YET_COVERED lists unknown `{name}`"
        );
    }
    let uncovered: Vec<_> = methods
        .iter()
        .filter(|m| !covered(m) && !NOT_YET_COVERED.contains(&m.as_str()))
        .collect();
    assert!(
        uncovered.is_empty(),
        "methods no conformance test calls (write one, or list in NOT_YET_COVERED): {uncovered:?}"
    );
    let stale: Vec<_> = NOT_YET_COVERED
        .iter()
        .filter(|n| methods.contains(**n) && covered(n))
        .collect();
    assert!(
        stale.is_empty(),
        "covered now, remove from NOT_YET_COVERED: {stale:?}"
    );
}
