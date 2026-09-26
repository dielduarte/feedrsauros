mod error;
mod events;
mod feeds;
mod folders;
mod items;
mod opml;
mod settings;
mod web;

use axum::Router;
use axum::routing::{get, patch, post, put};
use url::Url;

use crate::db::Db;
use crate::fetch::Fetcher;
use crate::poller::PollerHandle;

pub use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub fetcher: Fetcher,
    pub poller: PollerHandle,
    /// Where Jev is asked for judgments; tests point it at a stand-in.
    pub typesafe: Url,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/events", get(events::stream))
        .route("/api/sidebar", get(folders::sidebar))
        .route("/api/folders", post(folders::create))
        .route(
            "/api/folders/{slug}",
            patch(folders::rename).delete(folders::delete),
        )
        .route("/api/folders/{slug}/position", put(folders::move_to))
        .route("/api/feeds", post(feeds::subscribe))
        .route(
            "/api/feeds/{slug}",
            axum::routing::delete(feeds::unsubscribe),
        )
        .route("/api/feeds/{slug}/position", put(feeds::move_to))
        .route("/api/feeds/{slug}/title", put(feeds::rename))
        .route(
            "/api/feeds/{feed}/items/{item}",
            get(items::open).patch(items::update),
        )
        .route("/api/refresh", post(feeds::refresh))
        .route("/api/items", get(items::list))
        .route("/api/items/mark-read", post(items::mark_read))
        .route("/api/opml", get(opml::export).post(opml::import_file))
        .route("/api/settings", get(settings::show))
        .route(
            "/api/settings/api-key",
            put(settings::save_api_key).delete(settings::remove_api_key),
        )
        .route("/api/settings/ai", put(settings::set_ai))
        .fallback(web::serve)
        .with_state(state)
}
