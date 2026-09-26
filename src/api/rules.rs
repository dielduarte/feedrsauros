use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::{ApiError, AppState};
use crate::filter::filter_saved;
use crate::jev::Jev;
use crate::model::FeedScope;
use crate::poller::PollerEvent;
use crate::rules::Rule;

/// Enough for real filters, while keeping each article to one small request to Jev.
const MAX_RULES: usize = 20;

pub async fn list(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Vec<Rule>>, ApiError> {
    let feed = state.db.feed_id(&slug).await?;
    Ok(Json(state.db.rules(feed).await?))
}

pub async fn replace(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(rules): Json<Vec<Rule>>,
) -> Result<StatusCode, ApiError> {
    let feed = state.db.feed_id(&slug).await?;
    if rules.len() > MAX_RULES {
        return Err(ApiError::BadRequest(format!(
            "a feed can have at most {MAX_RULES} rules"
        )));
    }
    let rules: Vec<Rule> = rules
        .into_iter()
        .map(|rule| Rule {
            condition: rule.condition.trim().to_owned(),
            ..rule
        })
        .collect();
    if rules.iter().any(|rule| rule.condition.is_empty()) {
        return Err(ApiError::BadRequest(
            "every rule needs a description of the articles it applies to".into(),
        ));
    }
    if state.db.rules(feed).await? == rules {
        return Ok(StatusCode::NO_CONTENT);
    }
    state.db.set_rules(feed, &rules).await?;

    let jev = Jev::from_settings(&state.db, state.typesafe.clone()).await?;
    let (db, poller) = (state.db.clone(), state.poller.clone());
    tokio::spawn(async move {
        // New rules go over the articles already in the list once. This finishes before the
        // fetch below, which would otherwise store again what it takes out.
        if let Some(jev) = jev {
            match filter_saved(&db, &jev, feed).await {
                Ok(hidden) => poller.announce(PollerEvent::FeedFiltered { feed: slug, hidden }),
                Err(error) => tracing::warn!(%error, "could not apply new rules to saved articles"),
            }
        }
        // Articles the old rules kept out are judged again by the new ones on this fetch.
        if let Err(error) = poller.refresh(FeedScope::Feed(feed)).await {
            tracing::warn!(%error, "could not refresh a feed after its rules changed");
        }
    });
    Ok(StatusCode::NO_CONTENT)
}
