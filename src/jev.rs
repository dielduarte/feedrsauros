use std::time::Duration;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use url::Url;

use crate::db::{Db, DbError, Folder, SidebarFolder};
use crate::parse::ParsedFeed;
use crate::rules::Rule;

/// TypeSafe's evaluation endpoint, where Jev runs.
pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// Folder slugs are made of letters, digits and dashes, so this option can't clash with one.
const NO_FOLDER: &str = "~none";
/// A wrong pick costs a drag in the sidebar, so a fairly clear lead is enough to act on.
const MIN_CONFIDENCE: f64 = 0.5;
/// A rule matches when yes is more likely than no.
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

    /// Whether an article matches each rule's condition, in the rules' order.
    pub async fn matches(
        &self,
        rules: &[Rule],
        site: &str,
        article: Article<'_>,
    ) -> Result<Vec<bool>, JevError> {
        let reply: RuleReply = self.ask(&rules_request(rules, site, article)).await?;
        Ok((0..rules.len())
            .map(|i| {
                reply
                    .answers
                    .get(&rule_id(i))
                    .is_some_and(|answer| answer.noul >= MATCH)
            })
            .collect())
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

fn rule_id(index: usize) -> String {
    format!("rule_{index}")
}

/// What Jev reads of an article to judge it against rules.
#[derive(Clone, Copy)]
pub struct Article<'a> {
    pub title: Option<&'a str>,
    pub summary: Option<&'a str>,
}

fn rules_request(rules: &[Rule], site: &str, article: Article<'_>) -> Value {
    let questions: Map<String, Value> = rules
        .iter()
        .enumerate()
        .map(|(i, rule)| {
            let question = json!({
                "type": "noul",
                "instructions": {
                    "condition": rule.condition,
                    "question": "Does `article` match `condition`, the reader's description of a kind of article?",
                },
                "criteria": {
                    "true": "The article is the kind of article `condition` describes.",
                    "false": "The article is not that kind of article.",
                },
            });
            (rule_id(i), question)
        })
        .collect();
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
struct RuleReply {
    answers: std::collections::HashMap<String, NoulAnswer>,
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
