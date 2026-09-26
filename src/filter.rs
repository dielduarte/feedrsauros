use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::db::{Db, DbError, Feed};
use crate::jev::{Article, Jev};
use crate::model::FeedId;
use crate::parse::ParsedFeed;
use crate::rules::keeps;

/// Articles judged at once when new rules go over the whole list.
const CONCURRENCY: usize = 4;

/// Keeps out the new articles the feed's rules reject. Each article is judged once: whatever a
/// rule kept out is remembered, and whatever was kept is stored. If Jev can't be reached, the
/// article is kept, so AI never loses anything.
pub async fn filter_new(
    db: &Db,
    jev: Option<&Jev>,
    feed: &Feed,
    parsed: &mut ParsedFeed,
) -> Result<(), DbError> {
    let filtered = db.filtered_guids(feed.id).await?;
    parsed.items.retain(|item| !filtered.contains(&item.guid));
    let Some(jev) = jev else { return Ok(()) };
    let rules = db.rules(feed.id).await?;
    if rules.is_empty() {
        return Ok(());
    }

    let known = db.known_guids(feed.id).await?;
    let mut rejected: Vec<&str> = Vec::new();
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
            Ok(matched) if !keeps(&rules, &matched) => rejected.push(&article.guid),
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, feed = %feed.url, "could not check an article against the feed's rules");
            }
        }
    }
    db.remember_filtered(feed.id, &rejected).await?;
    let rejected: HashSet<String> = rejected.into_iter().map(str::to_owned).collect();
    parsed.items.retain(|item| !rejected.contains(&item.guid));
    Ok(())
}

/// Runs a feed's rules over the articles already in its list, once, when the rules change.
/// Returns how many were taken out. Starred articles stay; anything Jev can't judge stays too.
pub async fn filter_saved(db: &Db, jev: &Jev, feed: FeedId) -> Result<usize, DbError> {
    let rules = db.rules(feed).await?;
    if rules.is_empty() {
        return Ok(0);
    }
    let (site, articles) = db.unstarred_articles(feed).await?;
    let (rules, site) = (Arc::new(rules), Arc::new(site));
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

    let mut rejected = Vec::new();
    while let Some(joined) = checks.join_next().await {
        match joined {
            Ok((guid, Ok(false))) => rejected.push(guid),
            Ok((_, Ok(true))) => {}
            Ok((_, Err(error))) => {
                tracing::warn!(%error, "could not check a saved article against the feed's rules");
            }
            Err(error) => tracing::error!(%error, "rule check task failed"),
        }
    }
    db.hide_articles(feed, &rejected).await?;
    Ok(rejected.len())
}
