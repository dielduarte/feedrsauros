use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::db::{Db, DbError, Feed};
use crate::jev::{Article, Jev};
use crate::model::FeedId;
use crate::parse::ParsedFeed;

/// Articles judged at once when filters go over a whole feed.
const CONCURRENCY: usize = 4;

/// What the reader wants and doesn't want from a feed, in their own words. Either may be unset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filters {
    pub wanted: Option<String>,
    pub unwanted: Option<String>,
}

impl Filters {
    /// Blank text means no filter, so it's never sent to Jev as one.
    pub fn new(wanted: Option<&str>, unwanted: Option<&str>) -> Self {
        let clean = |text: Option<&str>| {
            text.map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        };
        Self {
            wanted: clean(wanted),
            unwanted: clean(unwanted),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.wanted.is_none() && self.unwanted.is_none()
    }
}

/// How an article fared against each filter that is set; `None` for a filter that isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub wanted: Option<bool>,
    pub unwanted: Option<bool>,
}

impl Verdict {
    /// Kept when it's something the reader wants (if they said) and nothing they don't want.
    pub fn keeps(self) -> bool {
        self.wanted.unwrap_or(true) && !self.unwanted.unwrap_or(false)
    }
}

/// Guids of the fetched articles the feed's filters reject, to hide once they're stored. Only
/// articles new to the feed are judged, and if Jev can't be reached an article is kept, so AI
/// never loses anything.
pub async fn rejected_new(
    db: &Db,
    jev: Option<&Jev>,
    feed: &Feed,
    parsed: &ParsedFeed,
) -> Result<Vec<String>, DbError> {
    let Some(jev) = jev else {
        return Ok(Vec::new());
    };
    let filters = db.filters(feed.id).await?;
    if filters.is_empty() {
        return Ok(Vec::new());
    }

    let known = db.known_guids(feed.id).await?;
    let mut rejected = Vec::new();
    for article in parsed
        .items
        .iter()
        .filter(|item| !known.contains(&item.guid))
    {
        let judged = Article {
            title: article.title.as_deref(),
            summary: article.summary.as_deref(),
        };
        match jev.judge(&filters, &parsed.title, judged).await {
            Ok(verdict) if !verdict.keeps() => rejected.push(article.guid.clone()),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, feed = %feed.url, "could not check an article against the feed's filters");
            }
        }
    }
    Ok(rejected)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Changed {
    pub hidden: u64,
    pub shown: u64,
}

/// Runs a feed's changed filters over every article it has stored, hidden ones included, and
/// hides or brings back whichever the new verdict differs for. Starred articles are left alone,
/// and an article Jev can't judge keeps its current state.
pub async fn reapply(db: &Db, jev: &Jev, feed: FeedId) -> Result<Changed, DbError> {
    let filters = db.filters(feed).await?;
    if filters.is_empty() {
        let shown = db.show_all(feed).await?;
        return Ok(Changed { hidden: 0, shown });
    }

    let (site, articles) = db.judgeable_articles(feed).await?;
    let (filters, site): (Arc<Filters>, Arc<str>) = (Arc::new(filters), site.into());
    let limit = Arc::new(Semaphore::new(CONCURRENCY));
    let mut checks = JoinSet::new();
    for article in articles {
        let (jev, filters, site, limit) =
            (jev.clone(), filters.clone(), site.clone(), limit.clone());
        checks.spawn(async move {
            let _slot = limit.acquire_owned().await;
            let judged = Article {
                title: article.title.as_deref(),
                summary: article.summary.as_deref(),
            };
            let verdict = jev.judge(&filters, &site, judged).await;
            (article.guid, verdict.map(Verdict::keeps))
        });
    }

    let (mut hide, mut show) = (Vec::new(), Vec::new());
    while let Some(joined) = checks.join_next().await {
        match joined {
            Ok((guid, Ok(false))) => hide.push(guid),
            Ok((guid, Ok(true))) => show.push(guid),
            Ok((_, Err(error))) => {
                tracing::warn!(%error, "could not check a saved article against the feed's filters");
            }
            Err(error) => tracing::error!(%error, "filter check task failed"),
        }
    }
    let (hidden, shown) = db.set_hidden(feed, &hide, &show).await?;
    Ok(Changed { hidden, shown })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(wanted: Option<bool>, unwanted: Option<bool>) -> Verdict {
        Verdict { wanted, unwanted }
    }

    #[test]
    fn keeps_everything_without_filters() {
        assert!(verdict(None, None).keeps());
    }

    #[test]
    fn keeps_only_what_the_reader_wants_when_they_say() {
        assert!(verdict(Some(true), None).keeps());
        assert!(!verdict(Some(false), None).keeps());
    }

    #[test]
    fn keeps_out_what_the_reader_does_not_want() {
        assert!(!verdict(None, Some(true)).keeps());
        assert!(verdict(None, Some(false)).keeps());
    }

    #[test]
    fn not_wanted_wins_over_wanted() {
        assert!(!verdict(Some(true), Some(true)).keeps());
    }

    #[test]
    fn treats_blank_text_as_no_filter() {
        assert!(Filters::new(Some("  "), Some("\n")).is_empty());
        assert_eq!(
            Filters::new(Some(" science "), None),
            Filters {
                wanted: Some("science".into()),
                unwanted: None
            }
        );
    }
}
