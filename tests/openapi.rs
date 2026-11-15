//! Guards the OpenAPI document the frontend client is generated from.

use einrad_server::http_server::routes::get_api_openapi;
use utoipa::openapi::HttpMethod;

const COMMANDS: &[&str] = &[
    "add_category",
    "add_final",
    "add_heat",
    "add_rider",
    "add_sponsor",
    "add_timeplan_entry",
    "apply_jury_remap",
    "create_booking",
    "create_club",
    "create_competition",
    "create_club_group",
    "delete_booking",
    "delete_category",
    "delete_club_group",
    "delete_rider",
    "delete_final",
    "delete_heat",
    "delete_timeplan_entry",
    "edit_booking",
    "edit_category",
    "edit_competition",
    "edit_club_act",
    "edit_rider",
    "set_single",
    "request_pair",
    "link_pair",
    "leave_pair",
    "dissolve_pair",
    "edit_final",
    "edit_heat",
    "edit_timeplan_entry",
    "import_competition",
    "login",
    "logout",
    "move_category_down",
    "move_category_up",
    "move_timeplan_down",
    "move_timeplan_up",
    "preview_club_mail",
    "preview_competition_import",
    "register",
    "reload_db",
    "create_db_backup",
    "delete_db_backups",
    "restore_db_backup",
    "clear_act_song",
    "remove_competition_logo",
    "rename_club",
    "rename_club_group",
    "replace_act_rider",
    "request_password_reset",
    "resend_mail_validation",
    "reset_block_order",
    "reset_password",
    "save_act_song",
    "send_club_mail",
    "set_act_music_started",
    "set_act_order",
    "set_block_order",
    "set_checked_in",
    "set_club_group_members",
    "set_club_group_type",
    "set_club_responsible",
    "set_competition_logo",
    "set_break_image",
    "set_image_source",
    "refresh_images",
    "remove_break_image",
    "remove_sponsor",
    "reorder_sponsors",
    "update_sponsor",
    "upload_sponsor_image",
    "set_final_placement",
    "set_fee_waived",
    "set_final_published",
    "set_heat_skip",
    "set_photo_hidden",
    "set_song_checked",
    "set_user_roles",
    "set_act_withdrawn",
    "timeplan_backward",
    "timeplan_forward",
    "verify_email",
    "edit_jury_unit",
    "move_judge_registrations",
    "resolve_unmapped_registration",
    "add_club_judge",
    "delete_club_judge",
    "edit_club_judge",
    "set_jury_deployments_override",
    "assign_judge",
    "set_jury_plan_published",
    "set_startlist_published",
    "unassign_judge",
];

const QUERIES: &[&str] = &[
    "export_competition",
    "get_act",
    "get_club",
    "get_club_mail_options",
    "get_competition_images",
    "get_competition_settings",
    "list_sponsors",
    "get_photos",
    "get_playback",
    "get_startlist_csv",
    "get_structure",
    "get_system_status",
    "list_acts",
    "list_booking_log",
    "list_bookings",
    "list_categories",
    "list_check_ins",
    "list_club_acts",
    "list_club_groups",
    "list_club_riders",
    "list_club_pair_requests",
    "list_clubs",
    "list_finals",
    "list_heats",
    "list_groups",
    "list_riders",
    "list_pair_requests",
    "list_timeplan",
    "list_users",
    "list_db_backups",
    "check_db_backup",
    "list_missing_songs",
    "predict_timeplan",
    "startlist",
    "whoami",
    "list_jury_units",
    "list_club_judges",
    "list_club_mails",
    "list_judges",
    "list_jury_plan",
];

#[test]
fn every_command_is_documented_as_post() {
    let openapi = get_api_openapi();
    for name in COMMANDS {
        let path = format!("/api/command/{name}");
        let item = openapi
            .paths
            .paths
            .get(&path)
            .unwrap_or_else(|| panic!("{path} missing from OpenAPI"));
        assert!(item.post.is_some(), "{path} is not a POST endpoint");
    }
}

#[test]
fn every_query_is_documented_as_get() {
    let openapi = get_api_openapi();
    for name in QUERIES {
        let path = format!("/api/query/{name}");
        let item = openapi
            .paths
            .paths
            .get(&path)
            .unwrap_or_else(|| panic!("{path} missing from OpenAPI"));
        assert!(item.get.is_some(), "{path} is not a GET endpoint");
    }
}

#[test]
fn openapi_contains_no_unexpected_paths() {
    let openapi = get_api_openapi();
    assert_eq!(openapi.paths.paths.len(), COMMANDS.len() + QUERIES.len());
}

#[test]
fn every_operation_has_tags_and_responses() {
    let openapi = get_api_openapi();
    for (path, item) in &openapi.paths.paths {
        for method in [HttpMethod::Get, HttpMethod::Post] {
            let operation = match method {
                HttpMethod::Get => item.get.as_ref(),
                _ => item.post.as_ref(),
            };
            if let Some(operation) = operation {
                assert!(
                    operation.tags.as_ref().is_some_and(|t| !t.is_empty()),
                    "{path} has no tags"
                );
                assert!(
                    !operation.responses.responses.is_empty(),
                    "{path} has no responses"
                );
            }
        }
    }
}

#[test]
fn every_route_is_called_by_an_integration_test() {
    let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let sources: Vec<String> = std::fs::read_dir(&tests_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "rs") && !path.ends_with("openapi.rs")
        })
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect();

    let untested: Vec<_> = get_api_openapi()
        .paths
        .paths
        .into_keys()
        .filter(|path| {
            // Match the path exactly, followed by a query string or closing quote.
            !sources.iter().any(|source| {
                source.contains(&format!("{path}\"")) || source.contains(&format!("{path}?"))
            })
        })
        .collect();
    assert!(untested.is_empty(), "routes without tests: {untested:#?}");
}
