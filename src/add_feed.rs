use chrono::{DateTime, Utc};
use url::Url;

use crate::db::{Db, DbError, Folder, NewFeed};
use crate::discover::{COMMON_FEED_PATHS, feed_links};
use crate::fetch::{FetchError, Fetched, Fetcher};
use crate::jev::Jev;
use crate::model::{FeedId, FolderId, Validators};
use crate::parse::{ParsedFeed, parse};
use crate::schedule::POLL_INTERVAL;

#[derive(Debug, serde::Serialize)]
pub struct Added {
    #[serde(skip)]
    pub id: FeedId,
    pub slug: String,
    pub title: String,
    pub new_items: u64,
    /// Slug of the folder Jev put the feed in, if it chose one.
    pub ai_folder: Option<String>,
}

/// Where a new feed goes.
pub enum Placement<'a> {
    Folder(FolderId),
    Unfiled,
    /// The folder Jev judges best, or unfiled if none fits or Jev can't be reached.
    BestFit(&'a Jev),
}

#[derive(Debug, thiserror::Error)]
pub enum AddFeedError {
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error("no feed found at this address")]
    NoFeedFound,
    #[error("already subscribed to this feed")]
    AlreadySubscribed,
    #[error(transparent)]
    Db(DbError),
}

impl From<DbError> for AddFeedError {
    fn from(error: DbError) -> Self {
        match error {
            DbError::AlreadyExists => Self::AlreadySubscribed,
            other => Self::Db(other),
        }
    }
}

/// Turns what someone typed into a web URL, assuming https when the scheme is left out.
pub fn parse_input(input: &str) -> Option<Url> {
    let input = input.trim();
    let url = match Url::parse(input) {
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            Url::parse(&format!("https://{input}")).ok()?
        }
        parsed => parsed.ok()?,
    };
    matches!(url.scheme(), "http" | "https").then_some(url)
}

/// Accepts either a feed URL or a web page that links to (or hosts) a feed.
pub async fn add_feed(
    db: &Db,
    fetcher: &Fetcher,
    url: &Url,
    placement: Placement<'_>,
    now: DateTime<Utc>,
) -> Result<Added, AddFeedError> {
    let page = fetcher.fetch_page(url, now).await?;
    if let Ok(feed) = parse(&page.body, &page.url, now) {
        return subscribe(db, page.url, feed, page.validators, placement, now).await;
    }

    let html = String::from_utf8_lossy(&page.body);
    let mut candidates = feed_links(&html, &page.url);
    for path in COMMON_FEED_PATHS {
        if let Ok(candidate) = page.url.join(path)
            && !candidates.contains(&candidate)
        {
            candidates.push(candidate);
        }
    }

    for candidate in candidates {
        if let Ok(Fetched::Updated {
            mut feed,
            validators,
            ..
        }) = fetcher.fetch(&candidate, &Validators::default(), now).await
        {
            feed.site_url.get_or_insert(page.url.clone());
            return subscribe(db, candidate, feed, validators, placement, now).await;
        }
    }
    Err(AddFeedError::NoFeedFound)
}

async fn subscribe(
    db: &Db,
    url: Url,
    feed: ParsedFeed,
    validators: Validators,
    placement: Placement<'_>,
    now: DateTime<Utc>,
) -> Result<Added, AddFeedError> {
    let (folder, ai_folder) = match placement {
        Placement::Folder(id) => (Some(id), None),
        Placement::Unfiled => (None, None),
        Placement::BestFit(jev) => match best_fit(db, jev, &feed, &url).await? {
            Some(folder) => (Some(folder.id), Some(folder.slug)),
            None => (None, None),
        },
    };
    let new = NewFeed {
        url,
        title: feed.title.clone(),
        site_url: feed.site_url.clone(),
        folder,
    };
    let (stored, new_items) = db
        .subscribe(new, &feed, validators, now, now + POLL_INTERVAL)
        .await?;
    Ok(Added {
        id: stored.id,
        slug: stored.slug,
        title: feed.title,
        new_items,
        ai_folder,
    })
}

/// Adding a feed never fails because of AI: if Jev can't help, the feed is simply left unfiled.
async fn best_fit(
    db: &Db,
    jev: &Jev,
    feed: &ParsedFeed,
    url: &Url,
) -> Result<Option<Folder>, AddFeedError> {
    let folders = db.sidebar().await?.folders;
    if folders.is_empty() {
        return Ok(None);
    }
    match jev.pick_folder(&folders, feed, url).await {
        Ok(folder) => Ok(folder),
        Err(error) => {
            tracing::warn!(%error, "could not ask Jev for a folder");
            Ok(None)
        }
    }
}
