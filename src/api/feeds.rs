use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::{ApiError, AppState};
use crate::add_feed::{Added, Placement, add_feed, parse_input};
use crate::jev::Jev;
use crate::model::FeedScope;

#[derive(Deserialize)]
pub struct Subscribe {
    url: String,
    /// Slug of the folder to put the feed in.
    folder: Option<String>,
}

pub async fn subscribe(
    State(state): State<AppState>,
    Json(body): Json<Subscribe>,
) -> Result<(StatusCode, Json<Added>), ApiError> {
    let url = parse_input(&body.url)
        .ok_or_else(|| ApiError::BadRequest(format!("not a web address: {}", body.url)))?;
    let jev = Jev::from_settings(&state.db, state.typesafe.clone()).await?;
    let placement = match (body.folder, &jev) {
        (Some(slug), _) => Placement::Folder(state.db.folder_id(&slug).await?),
        (None, Some(jev)) => Placement::BestFit(jev),
        (None, None) => Placement::Unfiled,
    };
    let added = add_feed(&state.db, &state.fetcher, &url, placement, Utc::now()).await?;
    Ok((StatusCode::CREATED, Json(added)))
}

pub async fn unsubscribe(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = state.db.feed_id(&slug).await?;
    state.db.delete_feed(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Position {
    /// Folder slug; `null` moves the feed out of any folder.
    folder: Option<String>,
    index: usize,
}

pub async fn move_to(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<Position>,
) -> Result<StatusCode, ApiError> {
    let id = state.db.feed_id(&slug).await?;
    let folder = match body.folder {
        Some(folder) => Some(state.db.folder_id(&folder).await?),
        None => None,
    };
    state.db.move_feed(id, folder, body.index).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Title {
    /// `null` goes back to the title the feed publishes.
    title: Option<String>,
}

/// A renamed feed or folder moves to a new URL; this tells the client where.
#[derive(Serialize)]
pub struct Renamed {
    pub slug: String,
}

pub async fn rename(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<Title>,
) -> Result<Json<Renamed>, ApiError> {
    let id = state.db.feed_id(&slug).await?;
    let slug = state.db.set_custom_title(id, body.title.as_deref()).await?;
    Ok(Json(Renamed { slug }))
}

#[derive(Deserialize)]
pub struct Refresh {
    feed: Option<String>,
    folder: Option<String>,
}

#[derive(Serialize)]
pub struct Scheduled {
    scheduled: u64,
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<Refresh>,
) -> Result<(StatusCode, Json<Scheduled>), ApiError> {
    let scope = match (body.feed, body.folder) {
        (None, None) => FeedScope::All,
        (Some(feed), None) => FeedScope::Feed(state.db.feed_id(&feed).await?),
        (None, Some(folder)) => FeedScope::Folder(state.db.folder_id(&folder).await?),
        (Some(_), Some(_)) => {
            return Err(ApiError::BadRequest(
                "refresh either a feed or a folder".into(),
            ));
        }
    };
    let scheduled = state.poller.refresh(scope).await?;
    Ok((StatusCode::ACCEPTED, Json(Scheduled { scheduled })))
}
