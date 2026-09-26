use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use crate::db::{Db, DbError, Folder, SidebarFolder};
use crate::parse::ParsedFeed;

/// TypeSafe's evaluation endpoint, where Jev runs.
pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// Folder slugs are made of letters, digits and dashes, so this option can't clash with one.
const NO_FOLDER: &str = "~none";
/// A wrong pick costs a drag in the sidebar, so a fairly clear lead is enough to act on.
const MIN_CONFIDENCE: f64 = 0.5;
const RECENT_ARTICLES: usize = 10;
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum JevError {
    #[error("could not reach TypeSafe: {0}")]
    Request(#[from] reqwest::Error),
    #[error("TypeSafe answered {0}")]
    Status(reqwest::StatusCode),
    #[error("unexpected answer from TypeSafe: {0}")]
    Answer(#[from] serde_json::Error),
}

/// Asks Jev which of the reader's folders a newly added site belongs in.
pub struct FolderPicker {
    client: reqwest::Client,
    endpoint: Url,
    api_key: String,
}

impl FolderPicker {
    /// Only while AI features are turned on, which requires a saved API key.
    pub async fn from_settings(db: &Db, endpoint: Url) -> Result<Option<Self>, DbError> {
        Ok(db.ai_api_key().await?.map(|api_key| Self {
            client: reqwest::Client::builder()
                .timeout(TIMEOUT)
                .build()
                .expect("a client with a timeout always builds"),
            endpoint,
            api_key,
        }))
    }

    /// `None` when no folder suits the site well enough.
    pub async fn pick(
        &self,
        folders: &[SidebarFolder],
        site: &ParsedFeed,
        address: &Url,
    ) -> Result<Option<Folder>, JevError> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&request(folders, site, address))?)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(JevError::Status(response.status()));
        }
        let reply: Reply = serde_json::from_slice(&response.bytes().await?)?;
        let answer = reply.answers.folder;
        if answer.confidence < MIN_CONFIDENCE {
            return Ok(None);
        }
        Ok(folders
            .iter()
            .find(|f| f.folder.slug == answer.choice)
            .map(|f| f.folder.clone()))
    }
}

fn request(folders: &[SidebarFolder], site: &ParsedFeed, address: &Url) -> Value {
    let mut options: Map<String, Value> = folders
        .iter()
        .map(|f| {
            let sites: Vec<&str> = f.feeds.iter().map(|feed| feed.title.as_str()).collect();
            (
                f.folder.slug.clone(),
                json!({ "folder": f.folder.name, "sites_already_in_it": sites }),
            )
        })
        .collect();
    options.insert(
        NO_FOLDER.into(),
        json!("None of these folders suits the site; it's better left outside any folder."),
    );
    let recent: Vec<&str> = site
        .items
        .iter()
        .filter_map(|item| item.title.as_deref())
        .take(RECENT_ARTICLES)
        .collect();

    json!({
        "model": "jev-latest",
        "state": {
            "site": {
                "title": site.title,
                "address": site.site_url.as_ref().unwrap_or(address),
                "recent_articles": recent,
            }
        },
        "questions": {
            "folder": {
                "type": "choice",
                "instructions": "The reader groups the sites they follow into folders. Which folder should `site` go in, judging by what it publishes? Each folder lists the sites already in it.",
                "criteria": options,
            }
        }
    })
}

#[derive(Deserialize)]
struct Reply {
    answers: Answers,
}

#[derive(Deserialize)]
struct Answers {
    folder: ChoiceAnswer,
}

#[derive(Deserialize)]
struct ChoiceAnswer {
    choice: String,
    confidence: f64,
}
