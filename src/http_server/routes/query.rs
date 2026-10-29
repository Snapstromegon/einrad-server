use utoipa_axum::{router::OpenApiRouter, routes};

mod get_act;
mod get_club;
mod get_photos;
mod get_playback;
mod get_startlist_csv;
mod get_system_status;
mod list_acts;
mod list_booking_log;
mod list_bookings;
mod list_categories;
mod list_check_ins;
mod list_club_acts;
mod list_club_groups;
mod list_club_judges;
mod list_club_pair_requests;
mod list_club_riders;
mod list_clubs;
mod list_finals;
mod list_groups;
mod list_judges;
mod list_jury_categories;
mod list_jury_plan;
mod list_pair_requests;
mod list_riders;
mod list_timeplan;
mod list_users;
pub(crate) mod predict_timeplan;
mod startlist;
mod whoami;

pub fn get_query_router() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_act::get_act))
        .routes(routes!(get_club::get_club))
        .routes(routes!(get_photos::get_photos))
        .routes(routes!(get_playback::get_playback))
        .routes(routes!(list_club_riders::list_club_riders))
        .routes(routes!(list_club_pair_requests::list_club_pair_requests))
        .routes(routes!(list_jury_categories::list_jury_categories))
        .routes(routes!(list_users::list_users))
        .routes(routes!(whoami::whoami))
        .routes(routes!(get_system_status::get_system_status))
        .routes(routes!(list_riders::list_riders))
        .routes(routes!(list_pair_requests::list_pair_requests))
        .routes(routes!(list_club_acts::list_club_acts))
        .routes(routes!(list_acts::list_acts))
        .routes(routes!(list_categories::list_categories))
        .routes(routes!(list_finals::list_finals))
        .routes(routes!(list_timeplan::list_timeplan))
        .routes(routes!(startlist::startlist))
        .routes(routes!(get_startlist_csv::get_startlist_csv))
        .routes(routes!(predict_timeplan::predict_timeplan))
        .routes(routes!(list_clubs::list_clubs))
        .routes(routes!(list_bookings::list_bookings))
        .routes(routes!(list_booking_log::list_booking_log))
        .routes(routes!(list_groups::list_groups))
        .routes(routes!(list_club_groups::list_club_groups))
        .routes(routes!(list_check_ins::list_check_ins))
        .routes(routes!(list_club_judges::list_club_judges))
        .routes(routes!(list_judges::list_judges))
        .routes(routes!(list_jury_plan::list_jury_plan))
}
