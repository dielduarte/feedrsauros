use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use crate::db::{Db, DbError, Folder, SidebarFolder};
use crate::filter::{Filters, Verdict};
use crate::parse::ParsedFeed;

/// TypeSafe's evaluation endpoint, where Jev runs.
pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// Folder slugs are made of letters, digits and dashes, so this option can't clash with one.
const NO_FOLDER: &str = "~none";
/// A wrong pick costs a drag in the sidebar, so a fairly clear lead is enough to act on.
const MIN_CONFIDENCE: f64 = 0.5;
/// A filter applies when yes is more likely than no.
const MATCH: f64 = 0.5;
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

/// TypeSafe's Jev, for the few judgments feedrsauros needs.
#[derive(Clone)]
pub struct Jev {
    client: reqwest::Client,
    endpoint: Url,
    api_key: String,
}

impl Jev {
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

    /// Which of the reader's folders a newly added site belongs in; `None` when no folder suits
    /// it well enough.
    pub async fn pick_folder(
        &self,
        folders: &[SidebarFolder],
        site: &ParsedFeed,
        address: &Url,
    ) -> Result<Option<Folder>, JevError> {
        let reply: Reply = self.ask(&folder_request(folders, site, address)).await?;
        let answer = reply.answers.folder;
        if answer.confidence < MIN_CONFIDENCE {
            return Ok(None);
        }
        Ok(folders
            .iter()
            .find(|f| f.folder.slug == answer.choice)
            .map(|f| f.folder.clone()))
    }

    /// How an article fares against each of the feed's filters that is set.
    pub async fn judge(
        &self,
        filters: &Filters,
        site: &str,
        article: Article<'_>,
    ) -> Result<Verdict, JevError> {
        let reply: FilterReply = self.ask(&filters_request(filters, site, article)).await?;
        let yes = |answer: Option<NoulAnswer>| answer.map(|a| a.noul >= MATCH);
        Ok(Verdict {
            wanted: filters.wanted.as_ref().and(yes(reply.answers.wanted)),
            unwanted: filters.unwanted.as_ref().and(yes(reply.answers.unwanted)),
        })
    }

    async fn ask<T: serde::de::DeserializeOwned>(&self, request: &Value) -> Result<T, JevError> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.api_key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(request)?)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(JevError::Status(response.status()));
        }
        Ok(serde_json::from_slice(&response.bytes().await?)?)
    }
}

/// What Jev reads of an article to judge it against a feed's filters.
#[derive(Clone, Copy)]
pub struct Article<'a> {
    pub title: Option<&'a str>,
    pub summary: Option<&'a str>,
}

/// One yes/no question per filter the reader wrote; an unset filter isn't asked about.
fn filters_request(filters: &Filters, site: &str, article: Article<'_>) -> Value {
    let mut questions = Map::new();
    if let Some(wanted) = &filters.wanted {
        questions.insert(
            "wanted".into(),
            json!({
                "type": "noul",
                "instructions": {
                    "wanted": wanted,
                    "question": "The reader described in `wanted` what they want to see from this site. Is `article` something they want to see?",
                },
                "criteria": {
                    "true": "The article fits what the reader wants to see.",
                    "false": "The article is not something the reader asked to see.",
                },
            }),
        );
    }
    if let Some(unwanted) = &filters.unwanted {
        questions.insert(
            "unwanted".into(),
            json!({
                "type": "noul",
                "instructions": {
                    "unwanted": unwanted,
                    "question": "The reader described in `unwanted` what they don't want to see from this site. Is `article` something they don't want to see?",
                },
                "criteria": {
                    "true": "The article is the kind of thing the reader doesn't want to see.",
                    "false": "The article is not something the reader ruled out.",
                },
            }),
        );
    }
    json!({
        "model": "jev-latest",
        "state": {
            "article": {
                "site": site,
                "title": article.title,
                "summary": article.summary,
            }
        },
        "questions": questions,
    })
}

fn folder_request(folders: &[SidebarFolder], site: &ParsedFeed, address: &Url) -> Value {
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
struct FilterReply {
    answers: FilterAnswers,
}

#[derive(Deserialize)]
struct FilterAnswers {
    wanted: Option<NoulAnswer>,
    unwanted: Option<NoulAnswer>,
}

#[derive(Deserialize)]
struct NoulAnswer {
    noul: f64,
}

#[derive(Deserialize)]
struct ChoiceAnswer {
    choice: String,
    confidence: f64,
}
