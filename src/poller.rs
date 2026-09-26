use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::{Notify, Semaphore, broadcast, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::db::{Db, DbError, Feed, FetchRecord};
use crate::fetch::{FetchError, Fetched, Fetcher};
use crate::filter;
use crate::jev::Jev;
use crate::model::FeedScope;
use crate::schedule::{Attempt, HISTORY, adaptive_interval, next_fetch_at};

pub const CONCURRENCY: usize = 8;
pub const PER_HOST: usize = 2;
const OFFLINE_RETRY: Duration = Duration::from_secs(60);
/// Sleeping is capped because the monotonic clock pauses while a laptop sleeps; re-reading the
/// wall-clock schedule every minute catches up soon after wake.
const MAX_SLEEP: Duration = Duration::from_secs(60);
const MIN_SLEEP: Duration = Duration::from_secs(1);
const PROBE_CANDIDATES: u32 = 10;
const CHANGE_CHECK: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PollerEvent {
    BatchStarted {
        feeds: usize,
    },
    /// `feed` is the feed's slug, the same one its URL uses.
    FeedRefreshed {
        feed: String,
        new_items: u64,
    },
    FeedFailed {
        feed: String,
        error: String,
    },
    BatchFinished {
        health: BatchHealth,
    },
    /// Changed rules hid or brought back articles the feed had already stored.
    FeedFiltered {
        feed: String,
        hidden: u64,
        shown: u64,
    },
    /// Something the stream didn't report changed the data, e.g. the CLI wrote to the database
    /// or this client fell behind; reload everything.
    Resync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchHealth {
    Online,
    /// Nothing answered, including a feed known to be healthy: the problem is our connection,
    /// so failures are postponed instead of counted against the feeds.
    Offline,
}

#[derive(Clone)]
pub struct PollerHandle {
    db: Db,
    wake: Arc<Notify>,
    /// Weak so the channel closes when the poller stops, ending open event streams and letting
    /// the server shut down instead of waiting on them forever.
    events: broadcast::WeakSender<PollerEvent>,
    active: watch::Sender<bool>,
}

impl PollerHandle {
    /// While inactive nothing is fetched or checked; a batch already running finishes first.
    /// Feeds that came due meanwhile are fetched as soon as it's active again.
    pub fn set_active(&self, active: bool) {
        self.active.send_replace(active);
    }

    /// Returns how many feeds were scheduled.
    pub async fn refresh(&self, scope: FeedScope) -> Result<u64, DbError> {
        let scheduled = self.db.mark_due(scope, Utc::now()).await?;
        self.wake();
        Ok(scheduled)
    }

    /// Tells open event streams about something that happened outside a batch.
    pub fn announce(&self, event: PollerEvent) {
        if let Some(events) = self.events.upgrade() {
            let _ = events.send(event);
        }
    }

    /// Makes the poller re-check the schedule now, e.g. after feeds were added as due.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// The receiver reports the channel as closed once the poller has stopped.
    pub fn subscribe(&self) -> broadcast::Receiver<PollerEvent> {
        match self.events.upgrade() {
            Some(events) => events.subscribe(),
            None => broadcast::channel(1).1,
        }
    }
}

/// `typesafe` is where Jev is asked to apply feed rules, while AI features are on.
pub fn spawn(
    db: Db,
    fetcher: Fetcher,
    typesafe: Url,
    cancel: CancellationToken,
) -> (PollerHandle, JoinHandle<()>) {
    let (events, _) = broadcast::channel(256);
    let wake = Arc::new(Notify::new());
    let (active, _) = watch::channel(true);
    let handle = PollerHandle {
        db: db.clone(),
        wake: wake.clone(),
        events: events.downgrade(),
        active: active.clone(),
    };
    let task = tokio::spawn(async move {
        tokio::join!(
            run(
                db.clone(),
                fetcher,
                typesafe,
                wake,
                events.clone(),
                active.subscribe(),
                cancel.clone()
            ),
            watch_other_writers(db, events, active.subscribe(), cancel),
        );
    });
    (handle, task)
}

/// Resolves once active, or never if cancelled first.
async fn until_active(active: &mut watch::Receiver<bool>, cancel: &CancellationToken) -> bool {
    tokio::select! {
        result = active.wait_for(|active| *active) => result.is_ok(),
        () = cancel.cancelled() => false,
    }
}

/// Only one process fetches from a database file at a time, so running the desktop app and
/// `serve` together doesn't fetch every feed twice. The lock is released when the process exits.
fn try_lock_polling(db: &Path) -> Option<File> {
    let mut path = db.as_os_str().to_owned();
    path.push(".poller.lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    file.try_lock().ok()?;
    Some(file)
}

async fn run(
    db: Db,
    fetcher: Fetcher,
    typesafe: Url,
    wake: Arc<Notify>,
    events: broadcast::Sender<PollerEvent>,
    mut active: watch::Receiver<bool>,
    cancel: CancellationToken,
) {
    let mut lock = None;
    loop {
        if !until_active(&mut active, &cancel).await {
            return;
        }
        if lock.is_none() {
            lock = try_lock_polling(db.path());
        }
        if lock.is_none() {
            // Another process is polling; try again later or when asked to refresh.
            tokio::select! {
                () = tokio::time::sleep(MAX_SLEEP) => {}
                () = wake.notified() => {}
                () = cancel.cancelled() => return,
            }
            continue;
        }

        let now = Utc::now();
        let batch = async {
            match db.feeds_due(now).await {
                Ok(due) if !due.is_empty() => {
                    // Read per batch, so turning AI on or off applies from the next one.
                    let jev = Jev::from_settings(&db, typesafe.clone())
                        .await
                        .unwrap_or_else(|error| {
                            tracing::warn!(%error, "could not read the AI settings");
                            None
                        });
                    run_batch(&db, &fetcher, jev.as_ref(), due, now, &events).await;
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "could not load due feeds"),
            }
        };
        // Dropping an in-flight batch aborts its fetches; writes are transactional, so the
        // database stays consistent.
        tokio::select! {
            () = batch => {}
            () = cancel.cancelled() => return,
        }

        tokio::select! {
            () = tokio::time::sleep(time_until_next_due(&db).await) => {}
            () = wake.notified() => {}
            () = cancel.cancelled() => return,
        }
    }
}

/// The UI only hears about what this process does, so writes from another process, like the CLI,
/// are announced as a resync.
async fn watch_other_writers(
    db: Db,
    events: broadcast::Sender<PollerEvent>,
    mut active: watch::Receiver<bool>,
    cancel: CancellationToken,
) {
    let mut seen = db.data_version().await.ok();
    loop {
        if !until_active(&mut active, &cancel).await {
            return;
        }
        match db.data_version().await {
            Ok(version) if seen.is_some_and(|seen| seen != version) => {
                seen = Some(version);
                let _ = events.send(PollerEvent::Resync);
            }
            Ok(version) => seen = Some(version),
            Err(error) => tracing::warn!(%error, "could not check for outside changes"),
        }
        tokio::select! {
            () = tokio::time::sleep(CHANGE_CHECK) => {}
            () = cancel.cancelled() => return,
        }
    }
}

async fn time_until_next_due(db: &Db) -> Duration {
    match db.next_due_at().await {
        Ok(Some(at)) => (at - Utc::now()).to_std().unwrap_or(Duration::ZERO),
        Ok(None) => MAX_SLEEP,
        Err(error) => {
            tracing::warn!(%error, "could not load the schedule");
            MAX_SLEEP
        }
    }
    .clamp(MIN_SLEEP, MAX_SLEEP)
}

/// Fetches concurrently, but stores results from this task only: SQLite has a single writer,
/// so funnelling writes here avoids lock contention between fetches.
/// `jev` applies each feed's rules to its new articles; without it rules are skipped.
pub async fn run_batch(
    db: &Db,
    fetcher: &Fetcher,
    jev: Option<&Jev>,
    feeds: Vec<Feed>,
    now: DateTime<Utc>,
    events: &broadcast::Sender<PollerEvent>,
) -> BatchHealth {
    let _ = events.send(PollerEvent::BatchStarted { feeds: feeds.len() });

    let global = Arc::new(Semaphore::new(CONCURRENCY));
    let mut hosts: HashMap<String, Arc<Semaphore>> = HashMap::new();
    let mut fetches = JoinSet::new();
    for feed in feeds {
        let host = hosts
            .entry(origin(&feed.url))
            .or_insert_with(|| Arc::new(Semaphore::new(PER_HOST)))
            .clone();
        let global = global.clone();
        let fetcher = fetcher.clone();
        fetches.spawn(async move {
            // Host first, so a task queued behind a busy host doesn't hold a global slot.
            let _host = host.acquire_owned().await;
            let _global = global.acquire_owned().await;
            let outcome = fetcher.fetch(&feed.url, &feed.validators, now).await;
            (feed, outcome)
        });
    }

    let mut reached_server = false;
    let mut failures = Vec::new();
    while let Some(joined) = fetches.join_next().await {
        let (feed, outcome) = match joined {
            Ok(result) => result,
            Err(error) => {
                tracing::error!(%error, "fetch task failed");
                continue;
            }
        };
        match outcome {
            Err(error) => {
                reached_server |= error.reached_server();
                failures.push((feed, error));
            }
            Ok(fetched) => {
                reached_server = true;
                if let Err(error) = record_success(db, jev, &feed, fetched, now, events).await {
                    tracing::warn!(%error, feed = %feed.url, "could not store fetch");
                }
            }
        }
    }

    let health =
        if failures.is_empty() || reached_server || probe(db, fetcher, &failures, now).await {
            BatchHealth::Online
        } else {
            BatchHealth::Offline
        };
    for (feed, error) in failures {
        if let Err(db_error) = record_failure(db, &feed, error, health, now, events).await {
            tracing::warn!(error = %db_error, feed = %feed.url, "could not store fetch failure");
        }
    }

    let _ = events.send(PollerEvent::BatchFinished { health });
    health
}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

/// When every fetch in a batch failed to reach its server, checks a feed on another server
/// that worked last time. If that one also fails, the problem is our connection.
async fn probe(
    db: &Db,
    fetcher: &Fetcher,
    failures: &[(Feed, FetchError)],
    now: DateTime<Utc>,
) -> bool {
    let failing: HashSet<String> = failures.iter().map(|(feed, _)| origin(&feed.url)).collect();
    let candidates = match db.healthy_feeds(PROBE_CANDIDATES).await {
        Ok(candidates) => candidates,
        Err(error) => {
            tracing::warn!(%error, "could not load probe candidates");
            return true;
        }
    };
    let Some(probe) = candidates
        .into_iter()
        .find(|f| !failing.contains(&origin(&f.url)))
    else {
        // Nothing to compare against, so the feeds get the blame.
        return true;
    };
    match fetcher.fetch(&probe.url, &probe.validators, now).await {
        Ok(_) => true,
        Err(error) => error.reached_server(),
    }
}

async fn record_success(
    db: &Db,
    jev: Option<&Jev>,
    feed: &Feed,
    fetched: Fetched,
    now: DateTime<Utc>,
    events: &broadcast::Sender<PollerEvent>,
) -> Result<(), DbError> {
    let new_items = match fetched {
        Fetched::Updated {
            feed: parsed,
            validators,
            moved_to,
        } => {
            // Judged before storing, while it's still clear which articles are new.
            let rejected = filter::rejected_new(db, jev, feed, &parsed).await?;
            let published: Vec<_> = parsed.items.iter().map(|i| i.published_at).collect();
            let next = next_fetch_at(
                Attempt::Succeeded,
                adaptive_interval(&published, now),
                feed.id,
                now,
            );
            let record = FetchRecord::Updated {
                feed: &parsed,
                validators,
            };
            let stored = db.record_fetch(feed.id, record, now, next).await?;
            let (hidden, _) = db.set_hidden(feed.id, &rejected, &[]).await?;
            if let Some(url) = moved_to {
                match db.update_feed_url(feed.id, &url).await {
                    Err(DbError::AlreadyExists) => {
                        tracing::warn!(from = %feed.url, to = %url, "feed moved to a URL that is already subscribed");
                    }
                    other => other?,
                }
            }
            stored.saturating_sub(hidden)
        }
        Fetched::NotModified => {
            let published = db.recent_publish_times(feed.id, HISTORY).await?;
            let next = next_fetch_at(
                Attempt::Succeeded,
                adaptive_interval(&published, now),
                feed.id,
                now,
            );
            db.record_fetch(feed.id, FetchRecord::NotModified, now, next)
                .await?
        }
    };
    let _ = events.send(PollerEvent::FeedRefreshed {
        feed: feed.slug.clone(),
        new_items,
    });
    Ok(())
}

async fn record_failure(
    db: &Db,
    feed: &Feed,
    error: FetchError,
    health: BatchHealth,
    now: DateTime<Utc>,
    events: &broadcast::Sender<PollerEvent>,
) -> Result<(), DbError> {
    if health == BatchHealth::Offline {
        db.record_fetch(feed.id, FetchRecord::Postponed, now, now + OFFLINE_RETRY)
            .await?;
        return Ok(());
    }

    let retry_after = match error {
        FetchError::RetryLater { retry_after } => retry_after,
        _ => None,
    };
    let published = db.recent_publish_times(feed.id, HISTORY).await?;
    let attempt = Attempt::Failed {
        consecutive_failures: feed.error_count + 1,
        retry_after,
    };
    let next = next_fetch_at(attempt, adaptive_interval(&published, now), feed.id, now);
    let message = error.to_string();
    let record = FetchRecord::Failed {
        error: message.clone(),
    };
    db.record_fetch(feed.id, record, now, next).await?;
    let _ = events.send(PollerEvent::FeedFailed {
        feed: feed.slug.clone(),
        error: message,
    });
    Ok(())
}
