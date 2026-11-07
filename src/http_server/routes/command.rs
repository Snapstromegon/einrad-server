use axum::extract::DefaultBodyLimit;
use utoipa_axum::{router::OpenApiRouter, routes};

mod add_category;
mod add_club_judge;
mod add_final;
mod add_jury_category;
mod add_rider;
mod add_timeplan_entry;
mod apply_jury_remap;
mod assign_judge;
mod create_booking;
mod create_club;
mod create_club_group;
mod delete_booking;
mod delete_category;
mod delete_club_group;
mod delete_club_judge;
mod delete_final;
mod delete_jury_category;
mod delete_rider;
mod delete_timeplan_entry;
mod dissolve_pair;
mod edit_booking;
mod edit_category;
mod edit_club_act;
mod edit_club_judge;
mod edit_competition;
mod edit_final;
mod edit_jury_category;
mod edit_rider;
mod edit_timeplan_entry;
mod import_competition;
mod leave_pair;
mod link_pair;
mod login;
mod logout;
mod move_category_down;
mod move_category_up;
mod move_timeplan_down;
mod move_timeplan_up;
mod preview_competition_import;
mod register;
mod reload_db;
mod remove_competition_logo;
mod rename_club;
mod rename_club_group;
mod replace_act_rider;
mod request_pair;
mod request_password_reset;
mod resend_mail_validation;
mod reset_block_order;
mod reset_password;
mod save_act_song;
mod set_act_music_started;
mod set_act_order;
mod set_act_withdrawn;
mod set_block_order;
mod set_checked_in;
mod set_club_group_members;
mod set_club_group_type;
mod set_club_responsible;
mod set_competition_logo;
mod set_fee_waived;
mod set_final_placement;
mod set_final_published;
mod set_jury_category_order;
mod set_jury_deployments_override;
mod set_jury_plan_published;
mod set_photo_hidden;
mod set_single;
mod set_song_checked;
mod set_user_roles;
mod timeplan_backward;
mod timeplan_forward;
mod unassign_judge;
mod verify_email;

pub fn get_command_router() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(add_jury_category::add_jury_category))
        .routes(routes!(delete_jury_category::delete_jury_category))
        .routes(routes!(edit_jury_category::edit_jury_category))
        .routes(routes!(register::register))
        .routes(routes!(set_jury_category_order::set_jury_category_order))
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
        .routes(routes!(set_final_placement::set_final_placement))
        .routes(routes!(set_final_published::set_final_published))
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
        .routes(routes!(import_competition::import_competition))
        .routes(routes!(edit_competition::edit_competition))
        .routes(routes!(set_competition_logo::set_competition_logo))
        .routes(routes!(remove_competition_logo::remove_competition_logo))
        .routes(routes!(apply_jury_remap::apply_jury_remap))
        .routes(routes!(add_club_judge::add_club_judge))
        .routes(routes!(delete_club_judge::delete_club_judge))
        .routes(routes!(edit_club_judge::edit_club_judge))
        .routes(routes!(
            set_jury_deployments_override::set_jury_deployments_override
        ))
        .routes(routes!(assign_judge::assign_judge))
        .routes(routes!(set_jury_plan_published::set_jury_plan_published))
        .routes(routes!(unassign_judge::unassign_judge))
        .layer(DefaultBodyLimit::max(save_act_song::MAX_REQUEST_SIZE))
}
