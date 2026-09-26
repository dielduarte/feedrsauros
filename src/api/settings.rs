use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::{ApiError, AppState};
use crate::db::DbError;

/// The key itself never leaves the server; only enough of it to recognise which one is saved.
#[derive(Serialize)]
pub struct Settings {
    ai_enabled: bool,
    api_key: Option<ApiKey>,
}

#[derive(Serialize)]
struct ApiKey {
    hint: String,
}

pub async fn show(State(state): State<AppState>) -> Result<Json<Settings>, ApiError> {
    let settings = state.db.settings().await?;
    Ok(Json(Settings {
        ai_enabled: settings.ai_enabled,
        api_key: settings.api_key_hint.map(|hint| ApiKey { hint }),
    }))
}

#[derive(Deserialize)]
pub struct NewApiKey {
    key: String,
}

pub async fn save_api_key(
    State(state): State<AppState>,
    Json(body): Json<NewApiKey>,
) -> Result<StatusCode, ApiError> {
    let key = body.key.trim();
    if key.is_empty() {
        return Err(ApiError::BadRequest("the API key is empty".into()));
    }
    state.db.set_api_key(key).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_api_key(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state.db.clear_api_key().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Ai {
    enabled: bool,
}

pub async fn set_ai(
    State(state): State<AppState>,
    Json(body): Json<Ai>,
) -> Result<StatusCode, ApiError> {
    match state.db.set_ai_enabled(body.enabled).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(DbError::NotFound) => Err(ApiError::BadRequest(
            "add a typesafe.ai API key before turning on AI features".into(),
        )),
        Err(error) => Err(error.into()),
    }
}
