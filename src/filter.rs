use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::db::{Db, DbError, Feed};
use crate::jev::{Article, Jev};
use crate::model::FeedId;
use crate::parse::ParsedFeed;
use crate::rules::{Rule, keeps};

/// Articles judged at once when rules go over a whole feed.
const CONCURRENCY: usize = 4;

/// Guids of the fetched articles the feed's rules reject, to hide once they're stored. Only
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
    let rules = db.rules(feed.id).await?;
    if rules.is_empty() {
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
        match jev.matches(&rules, &parsed.title, judged).await {
            Ok(matched) if !keeps(&rules, &matched) => rejected.push(article.guid.clone()),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, feed = %feed.url, "could not check an article against the feed's rules");
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

/// Runs a feed's changed rules over every article it has stored, hidden ones included, and
/// hides or brings back whichever the new verdict differs for. Starred articles are left alone,
/// and an article Jev can't judge keeps its current state.
pub async fn reapply(db: &Db, jev: &Jev, feed: FeedId) -> Result<Changed, DbError> {
    let rules = db.rules(feed).await?;
    if rules.is_empty() {
        let shown = db.show_all(feed).await?;
        return Ok(Changed { hidden: 0, shown });
    }

    let (site, articles) = db.judgeable_articles(feed).await?;
    let (rules, site): (Arc<[Rule]>, Arc<str>) = (rules.into(), site.into());
    let limit = Arc::new(Semaphore::new(CONCURRENCY));
    let mut checks = JoinSet::new();
    for article in articles {
        let (jev, rules, site, limit) = (jev.clone(), rules.clone(), site.clone(), limit.clone());
        checks.spawn(async move {
            let _slot = limit.acquire_owned().await;
            let judged = Article {
                title: article.title.as_deref(),
                summary: article.summary.as_deref(),
            };
            let verdict = jev.matches(&rules, &site, judged).await;
            (article.guid, verdict.map(|matched| keeps(&rules, &matched)))
        });
    }

    let (mut hide, mut show) = (Vec::new(), Vec::new());
    while let Some(joined) = checks.join_next().await {
        match joined {
            Ok((guid, Ok(false))) => hide.push(guid),
            Ok((guid, Ok(true))) => show.push(guid),
            Ok((_, Err(error))) => {
                tracing::warn!(%error, "could not check a saved article against the feed's rules");
            }
            Err(error) => tracing::error!(%error, "rule check task failed"),
        }
    }
    let (hidden, shown) = db.set_hidden(feed, &hide, &show).await?;
    Ok(Changed { hidden, shown })
}
