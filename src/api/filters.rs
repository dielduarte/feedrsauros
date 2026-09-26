use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use super::{ApiError, AppState};
use crate::filter::{Changed, Filters, reapply};
use crate::jev::Jev;
use crate::poller::PollerEvent;

pub async fn show(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Filters>, ApiError> {
    let feed = state.db.feed_id(&slug).await?;
    Ok(Json(state.db.filters(feed).await?))
}

#[derive(Deserialize)]
pub struct NewFilters {
    wanted: Option<String>,
    unwanted: Option<String>,
}

pub async fn save(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<NewFilters>,
) -> Result<StatusCode, ApiError> {
    let feed = state.db.feed_id(&slug).await?;
    let filters = Filters::new(body.wanted.as_deref(), body.unwanted.as_deref());
    if state.db.filters(feed).await? == filters {
        return Ok(StatusCode::NO_CONTENT);
    }
    state.db.set_filters(feed, &filters).await?;

    let jev = Jev::from_settings(&state.db, state.typesafe.clone()).await?;
    let (db, poller) = (state.db.clone(), state.poller.clone());
    tokio::spawn(async move {
        // With no filters left everything comes back, which needs no judging. Otherwise the new
        // filters go over the whole feed once, while AI features are on.
        let changed = match (filters.is_empty(), jev) {
            (true, _) => db
                .show_all(feed)
                .await
                .map(|shown| Changed { hidden: 0, shown }),
            (false, Some(jev)) => reapply(&db, &jev, feed).await,
            (false, None) => return,
        };
        match changed {
            Ok(Changed { hidden, shown }) => poller.announce(PollerEvent::FeedFiltered {
                feed: slug,
                hidden,
                shown,
            }),
            Err(error) => {
                tracing::warn!(%error, "could not apply changed filters to saved articles")
            }
        }
    });
    Ok(StatusCode::NO_CONTENT)
}
