//! `/api/openapi.json` — OpenAPI schema endpoint.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

use crate::error::ServerResult;
use crate::state::AppState;

/// Combined OpenAPI doc.
#[derive(OpenApi)]
#[openapi(paths(
    crate::routes::instance::get_instance,
    crate::routes::instance::get_path,
    crate::routes::instance::dispose,
    crate::routes::instance::get_vcs,
    crate::routes::config::get_config,
    crate::routes::config::update_config,
    crate::routes::config::providers,
    crate::routes::session::list,
    crate::routes::session::create,
    crate::routes::session::get_session,
    crate::routes::session::delete_session,
    crate::routes::session::abort,
    crate::routes::session::messages,
    crate::routes::session::append_message,
    crate::routes::file::content,
    crate::routes::file::list,
    crate::routes::file::find,
    crate::routes::file::find_text,
    crate::routes::daemon::list,
    crate::routes::daemon::preview,
    crate::routes::daemon::get_run,
    crate::routes::daemon::events,
    crate::routes::daemon::tasks,
    crate::routes::daemon::pause,
    crate::routes::daemon::resume,
    crate::routes::daemon::abort,
    crate::routes::sync::history,
    crate::routes::sync::start,
    crate::routes::sync::replay,
    crate::routes::tui::append_prompt,
    crate::routes::tui::open_help,
    crate::routes::tui::open_sessions,
    crate::routes::tui::open_themes,
    crate::routes::tui::open_models,
    crate::routes::tui::submit_prompt,
    crate::routes::tui::clear_prompt,
    crate::routes::tui::execute_command,
    crate::routes::tui::show_toast,
    crate::routes::tui::select_session,
    crate::routes::provider::list,
    crate::routes::provider::auth_methods,
    crate::routes::provider::authorize_get,
    crate::routes::permission::list,
    crate::routes::permission::reply,
    crate::routes::question::list,
    crate::routes::question::get_question,
    crate::routes::question::answer,
    crate::routes::mcp::list,
    crate::routes::mcp::get_server,
    crate::routes::workspace::list,
    crate::routes::workspace::create,
    crate::routes::workspace::get_workspace,
    crate::routes::workspace::remove,
    crate::routes::experimental::list,
    crate::routes::experimental::get_flag,
    crate::routes::experimental::set_flag,
    crate::routes::events::stream,
    crate::routes::ws::handler,
    crate::routes::pty::handler,
    crate::routes::v2::session::list,
    crate::routes::v2::session::prompt,
    crate::routes::v2::session::compact,
    crate::routes::v2::session::wait,
    crate::routes::v2::message::list,
))]
pub struct ApiDoc;

/// Build the OpenAPI router.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/openapi.json", get(openapi))
}

/// `GET /api/openapi.json` — emit the OpenAPI 3.0 spec.
pub async fn openapi(State(_state): State<Arc<AppState>>) -> ServerResult<Json<serde_json::Value>> {
    let doc = ApiDoc::openapi();
    Ok(Json(serde_json::to_value(&doc)?))
}
