use axum::extract::DefaultBodyLimit;
use utoipa_axum::{router::OpenApiRouter, routes};

mod add_category;
mod add_club_judge;
mod add_final;
mod add_heat;
mod add_rider;
mod add_sponsor;
mod add_timeplan_entry;
mod apply_jury_remap;
mod assign_judge;
mod create_booking;
mod create_club;
mod create_club_group;
mod create_competition;
mod delete_booking;
mod delete_category;
mod delete_club_group;
mod delete_club_judge;
mod delete_final;
mod delete_heat;
mod delete_rider;
mod delete_timeplan_entry;
mod dissolve_pair;
mod edit_booking;
mod edit_category;
mod edit_club_act;
mod edit_club_judge;
mod edit_competition;
mod edit_final;
mod edit_heat;
mod edit_jury_unit;
mod edit_rider;
mod edit_timeplan_entry;
mod image_support;
mod import_competition;
mod leave_pair;
mod link_pair;
mod login;
mod logout;
mod move_category_down;
mod move_category_up;
mod move_judge_registrations;
mod move_timeplan_down;
mod move_timeplan_up;
mod preview_club_mail;
mod preview_competition_import;
mod refresh_images;
mod register;
mod reload_db;
mod remove_break_image;
mod remove_competition_logo;
mod remove_sponsor;
mod rename_club;
mod rename_club_group;
mod reorder_sponsors;
mod replace_act_rider;
mod request_pair;
mod request_password_reset;
mod resend_mail_validation;
mod reset_block_order;
mod reset_password;
mod resolve_unmapped_registration;
mod save_act_song;
mod send_club_mail;
mod set_act_music_started;
mod set_act_order;
mod set_act_withdrawn;
mod set_block_order;
mod set_break_image;
mod set_checked_in;
mod set_club_group_members;
mod set_club_group_type;
mod set_club_responsible;
mod set_competition_logo;
mod set_fee_waived;
mod set_final_placement;
mod set_final_published;
mod set_heat_skip;
mod set_image_source;
mod set_jury_deployments_override;
mod set_jury_plan_published;
mod set_photo_hidden;
mod set_single;
mod set_song_checked;
mod set_startlist_published;
mod set_user_roles;
mod timeplan_backward;
mod timeplan_forward;
mod unassign_judge;
mod update_sponsor;
mod upload_sponsor_image;
mod verify_email;

pub fn get_command_router() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(edit_jury_unit::edit_jury_unit))
        .routes(routes!(move_judge_registrations::move_judge_registrations))
        .routes(routes!(
            resolve_unmapped_registration::resolve_unmapped_registration
        ))
        .routes(routes!(register::register))
        .routes(routes!(verify_email::verify_email))
        .routes(routes!(resend_mail_validation::resend_mail_validation))
        .routes(routes!(request_password_reset::request_password_reset))
        .routes(routes!(reset_password::reset_password))
        .routes(routes!(login::login))
        .routes(routes!(logout::logout))
        .routes(routes!(add_category::add_category))
        .routes(routes!(add_final::add_final))
        .routes(routes!(edit_final::edit_final))
        .routes(routes!(delete_final::delete_final))
        .routes(routes!(add_heat::add_heat))
        .routes(routes!(edit_heat::edit_heat))
        .routes(routes!(delete_heat::delete_heat))
        .routes(routes!(set_final_placement::set_final_placement))
        .routes(routes!(set_final_published::set_final_published))
        .routes(routes!(set_heat_skip::set_heat_skip))
        .routes(routes!(edit_category::edit_category))
        .routes(routes!(delete_category::delete_category))
        .routes(routes!(move_category_up::move_category_up))
        .routes(routes!(move_category_down::move_category_down))
        .routes(routes!(add_timeplan_entry::add_timeplan_entry))
        .routes(routes!(edit_timeplan_entry::edit_timeplan_entry))
        .routes(routes!(delete_timeplan_entry::delete_timeplan_entry))
        .routes(routes!(create_club::create_club))
        .routes(routes!(rename_club::rename_club))
        .routes(routes!(add_rider::add_rider))
        .routes(routes!(delete_rider::delete_rider))
        .routes(routes!(edit_rider::edit_rider))
        .routes(routes!(set_single::set_single))
        .routes(routes!(request_pair::request_pair))
        .routes(routes!(link_pair::link_pair))
        .routes(routes!(leave_pair::leave_pair))
        .routes(routes!(dissolve_pair::dissolve_pair))
        .routes(routes!(create_club_group::create_club_group))
        .routes(routes!(set_club_group_members::set_club_group_members))
        .routes(routes!(set_club_group_type::set_club_group_type))
        .routes(routes!(set_club_responsible::set_club_responsible))
        .routes(routes!(rename_club_group::rename_club_group))
        .routes(routes!(delete_club_group::delete_club_group))
        .routes(routes!(edit_club_act::edit_club_act))
        .routes(routes!(replace_act_rider::replace_act_rider))
        .routes(routes!(save_act_song::save_act_song))
        .routes(routes!(create_booking::create_booking))
        .routes(routes!(edit_booking::edit_booking))
        .routes(routes!(delete_booking::delete_booking))
        .routes(routes!(set_photo_hidden::set_photo_hidden))
        .routes(routes!(set_song_checked::set_song_checked))
        .routes(routes!(set_user_roles::set_user_roles))
        .routes(routes!(set_act_withdrawn::set_act_withdrawn))
        .routes(routes!(set_fee_waived::set_fee_waived))
        .routes(routes!(set_act_order::set_act_order))
        .routes(routes!(set_act_music_started::set_act_music_started))
        .routes(routes!(set_checked_in::set_checked_in))
        .routes(routes!(timeplan_forward::timeplan_forward))
        .routes(routes!(timeplan_backward::timeplan_backward))
        .routes(routes!(move_timeplan_up::move_timeplan_up))
        .routes(routes!(move_timeplan_down::move_timeplan_down))
        .routes(routes!(set_block_order::set_block_order))
        .routes(routes!(reset_block_order::reset_block_order))
        .routes(routes!(reload_db::reload_db))
        .routes(routes!(
            preview_competition_import::preview_competition_import
        ))
        .routes(routes!(create_competition::create_competition))
        .routes(routes!(import_competition::import_competition))
        .routes(routes!(edit_competition::edit_competition))
        .routes(routes!(remove_competition_logo::remove_competition_logo))
        .routes(routes!(remove_break_image::remove_break_image))
        .routes(routes!(set_image_source::set_image_source))
        .routes(routes!(refresh_images::refresh_images))
        .routes(routes!(add_sponsor::add_sponsor))
        .routes(routes!(update_sponsor::update_sponsor))
        .routes(routes!(remove_sponsor::remove_sponsor))
        .routes(routes!(reorder_sponsors::reorder_sponsors))
        .routes(routes!(apply_jury_remap::apply_jury_remap))
        .routes(routes!(add_club_judge::add_club_judge))
        .routes(routes!(delete_club_judge::delete_club_judge))
        .routes(routes!(edit_club_judge::edit_club_judge))
        .routes(routes!(
            set_jury_deployments_override::set_jury_deployments_override
        ))
        .routes(routes!(assign_judge::assign_judge))
        .routes(routes!(set_jury_plan_published::set_jury_plan_published))
        .routes(routes!(set_startlist_published::set_startlist_published))
        .routes(routes!(unassign_judge::unassign_judge))
        .routes(routes!(send_club_mail::send_club_mail))
        .routes(routes!(preview_club_mail::preview_club_mail))
        .layer(DefaultBodyLimit::max(save_act_song::MAX_REQUEST_SIZE))
        // Image uploads have no limit (ADR 0010): they're scaled down.
        .merge(
            OpenApiRouter::new()
                .routes(routes!(set_competition_logo::set_competition_logo))
                .routes(routes!(set_break_image::set_break_image))
                .routes(routes!(upload_sponsor_image::upload_sponsor_image))
                .layer(DefaultBodyLimit::disable()),
        )
}
