use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use super::{ApiError, AppState};
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
    state.db.set_rules(feed, &rules).await?;
    Ok(StatusCode::NO_CONTENT)
}
