use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, TimeZone, Utc};
use feedrsauros::db::{Db, Feed, FetchRecord, ItemQuery, ItemScope, NewFeed};
use feedrsauros::fetch::Fetcher;
use feedrsauros::filter::Filters;
use feedrsauros::jev::Jev;
use feedrsauros::model::{FeedId, FeedScope};
use feedrsauros::poller::{self, BatchHealth, PER_HOST, PollerEvent, run_batch};
use feedrsauros::schedule::POLL_INTERVAL;
use serde_json::{Map, Value, json};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Default)]
struct Traffic {
    requests: AtomicUsize,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    jev_requests: AtomicUsize,
    jev_down: AtomicBool,
}

struct Env {
    db: Db,
    fetcher: Fetcher,
    base: Url,
    traffic: Arc<Traffic>,
    /// A stand-in for TypeSafe's API, served next to the feeds.
    jev: Url,
    _dir: TempDir,
}

fn rss2() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/rss2.xml"
    ))
    .unwrap()
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 23, 12, 0, 0).unwrap()
}

async fn feed(State(traffic): State<Arc<Traffic>>, Path(_name): Path<String>) -> Vec<u8> {
    traffic.requests.fetch_add(1, Ordering::SeqCst);
    rss2()
}

async fn slow_feed(State(traffic): State<Arc<Traffic>>, Path(_name): Path<String>) -> Vec<u8> {
    traffic.requests.fetch_add(1, Ordering::SeqCst);
    let current = traffic.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    traffic.max_in_flight.fetch_max(current, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(100)).await;
    traffic.in_flight.fetch_sub(1, Ordering::SeqCst);
    rss2()
}

/// Says every rule matches "First post" and nothing else.
async fn fake_jev(State(traffic): State<Arc<Traffic>>, Json(body): Json<Value>) -> Response {
    traffic.jev_requests.fetch_add(1, Ordering::SeqCst);
    if traffic.jev_down.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let hit = body["state"]["article"]["title"] == "First post";
    let answers: Map<String, Value> = body["questions"]
        .as_object()
        .unwrap()
        .keys()
        .map(|id| {
            let yes = if hit { 0.97 } else { 0.03 };
            (id.clone(), json!({ "type": "noul", "noul": yes }))
        })
        .collect();
    Json(json!({ "model": "jev-1.13.0", "answers": answers, "usage": { "input_tokens": 1, "output_tokens": 1 } }))
        .into_response()
}

async fn env() -> Env {
    let traffic = Arc::new(Traffic::default());
    let app = Router::new()
        .route("/feeds/{name}", get(feed))
        .route("/slow/{name}", get(slow_feed))
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
        .route("/old", get(|| async { Redirect::permanent("/feeds/new") }))
        .route("/v1/systemone", post(fake_jev))
        .with_state(traffic.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    Env {
        db: Db::open(&dir.path().join("feedrsauros.db")).await.unwrap(),
        fetcher: Fetcher::new(Duration::from_secs(5)),
        jev: base.join("v1/systemone").unwrap(),
        base,
        traffic,
        _dir: dir,
    }
}

async fn unreachable_url() -> Url {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    Url::parse(&format!("http://{addr}/feed.xml")).unwrap()
}

impl Env {
    async fn add(&self, url: Url, at: DateTime<Utc>) -> Feed {
        let new = NewFeed {
            url: url.clone(),
            title: url.to_string(),
            site_url: None,
            folder: None,
        };
        self.db.insert_feed(new, at).await.unwrap()
    }

    async fn add_path(&self, path: &str) -> Feed {
        self.add(self.base.join(path).unwrap(), now()).await
    }

    /// A feed whose last fetch succeeded, so the poller may use it to check connectivity.
    async fn add_healthy(&self, url: Url) -> Feed {
        let feed = self.add(url, now()).await;
        self.db
            .record_fetch(
                feed.id,
                FetchRecord::NotModified,
                now(),
                now() + POLL_INTERVAL,
            )
            .await
            .unwrap();
        feed
    }

    async fn batch(&self, feeds: Vec<Feed>) -> (BatchHealth, Vec<PollerEvent>) {
        let (events, mut received) = broadcast::channel(64);
        let jev = Jev::from_settings(&self.db, self.jev.clone())
            .await
            .unwrap();
        let health = run_batch(&self.db, &self.fetcher, jev.as_ref(), feeds, now(), &events).await;
        let mut all = Vec::new();
        while let Ok(event) = received.try_recv() {
            all.push(event);
        }
        (health, all)
    }

    async fn turn_on_ai(&self) {
        self.db.set_api_key("ts_test_key").await.unwrap();
        self.db.set_ai_enabled(true).await.unwrap();
    }

    async fn hide_first_posts(&self, feed: FeedId) {
        let filters = Filters::new(None, Some("first posts"));
        self.db.set_filters(feed, &filters).await.unwrap();
    }

    async fn titles(&self, feed: FeedId) -> Vec<String> {
        let query = ItemQuery {
            scope: ItemScope::Feed(feed),
            unread_only: false,
            cursor: None,
            limit: 50,
        };
        let mut titles: Vec<String> = self
            .db
            .list_items(query)
            .await
            .unwrap()
            .items
            .into_iter()
            .filter_map(|item| item.title)
            .collect();
        titles.sort();
        titles
    }

    fn jev_requests(&self) -> usize {
        self.traffic.jev_requests.load(Ordering::SeqCst)
    }

    async fn stored(&self, id: FeedId) -> Feed {
        let far_future = now() + Duration::from_secs(365 * 24 * 3600);
        self.db
            .feeds_due(far_future)
            .await
            .unwrap()
            .into_iter()
            .find(|f| f.id == id)
            .unwrap()
    }
}

async fn eventually(what: &str, check: impl AsyncFn() -> bool) {
    for _ in 0..100 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting until {what}");
}

mod batch {
    use super::*;

    #[tokio::test]
    async fn stores_new_items_and_reports_them() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;

        let (health, events) = env.batch(vec![feed.clone()]).await;

        assert_eq!(health, BatchHealth::Online);
        assert_eq!(
            events,
            [
                PollerEvent::BatchStarted { feeds: 1 },
                PollerEvent::FeedRefreshed {
                    feed: feed.slug.clone(),
                    new_items: 4
                },
                PollerEvent::BatchFinished {
                    health: BatchHealth::Online
                },
            ]
        );
        assert_eq!(env.db.sidebar().await.unwrap().uncategorized[0].unread, 4);
    }

    #[tokio::test]
    async fn schedules_the_next_check_from_publishing_cadence() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;

        env.batch(vec![feed.clone()]).await;

        // The fixture's dated posts are ~a day apart, far slower than the default interval.
        assert!(env.stored(feed.id).await.next_fetch_at > now() + POLL_INTERVAL * 4);
    }

    #[tokio::test]
    async fn counts_failures_when_other_feeds_respond() {
        let env = env().await;
        let ok = env.add_path("feeds/a").await;
        let missing = env.add_path("missing").await;

        let (_, events) = env.batch(vec![ok, missing.clone()]).await;

        let stored = env.stored(missing.id).await;
        assert_eq!(stored.error_count, 1);
        assert_eq!(
            stored.last_error.as_deref(),
            Some("server responded with 404")
        );
        assert!(events.contains(&PollerEvent::FeedFailed {
            feed: missing.slug.clone(),
            error: "server responded with 404".to_string()
        }));
    }

    #[tokio::test]
    async fn blames_an_unreachable_feed_when_a_healthy_feed_responds() {
        let env = env().await;
        env.add_healthy(env.base.join("feeds/healthy").unwrap())
            .await;
        let dead = env.add(unreachable_url().await, now()).await;

        let (health, _) = env.batch(vec![dead.clone()]).await;

        assert_eq!(health, BatchHealth::Online);
        assert_eq!(env.stored(dead.id).await.error_count, 1);
    }

    #[tokio::test]
    async fn postpones_without_blame_when_offline() {
        let env = env().await;
        env.add_healthy(unreachable_url().await).await;
        let dead = env.add(unreachable_url().await, now()).await;

        let (health, events) = env.batch(vec![dead.clone()]).await;

        assert_eq!(health, BatchHealth::Offline);
        let stored = env.stored(dead.id).await;
        assert_eq!(stored.error_count, 0);
        assert!(stored.next_fetch_at > now());
        assert!(stored.next_fetch_at <= now() + Duration::from_secs(60));
        assert_eq!(
            events.last(),
            Some(&PollerEvent::BatchFinished {
                health: BatchHealth::Offline
            })
        );
    }

    #[tokio::test]
    async fn follows_feeds_that_moved_permanently() {
        let env = env().await;
        let feed = env.add_path("old").await;

        env.batch(vec![feed.clone()]).await;

        assert_eq!(
            env.stored(feed.id).await.url,
            env.base.join("feeds/new").unwrap()
        );
    }

    #[tokio::test]
    async fn limits_concurrent_requests_per_host() {
        let env = env().await;
        let mut feeds = Vec::new();
        for n in 0..6 {
            feeds.push(env.add_path(&format!("slow/{n}")).await);
        }

        env.batch(feeds).await;

        assert_eq!(env.traffic.max_in_flight.load(Ordering::SeqCst), PER_HOST);
    }
}

mod background {
    use super::*;

    async fn start(
        env: &Env,
    ) -> (
        poller::PollerHandle,
        CancellationToken,
        tokio::task::JoinHandle<()>,
    ) {
        let cancel = CancellationToken::new();
        let (handle, task) = poller::spawn(
            env.db.clone(),
            env.fetcher.clone(),
            env.jev.clone(),
            cancel.clone(),
        );
        (handle, cancel, task)
    }

    #[tokio::test]
    async fn polls_due_feeds() {
        let env = env().await;
        env.add(env.base.join("feeds/a").unwrap(), Utc::now()).await;

        let (_handle, cancel, _task) = start(&env).await;

        eventually("the feed is fetched", async || {
            env.db.sidebar().await.unwrap().uncategorized[0].unread == 4
        })
        .await;
        cancel.cancel();
    }

    #[tokio::test]
    async fn refresh_fetches_a_feed_that_is_not_due() {
        let env = env().await;
        let feed = env.add(env.base.join("feeds/a").unwrap(), Utc::now()).await;
        let (handle, cancel, _task) = start(&env).await;
        eventually("the first fetch is stored", async || {
            env.db
                .next_due_at()
                .await
                .unwrap()
                .is_some_and(|at| at > Utc::now())
        })
        .await;
        let mut events = handle.subscribe();

        assert_eq!(handle.refresh(FeedScope::Feed(feed.id)).await.unwrap(), 1);

        eventually("the refresh fetch", async || {
            env.traffic.requests.load(Ordering::SeqCst) == 2
        })
        .await;
        let refreshed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let PollerEvent::FeedRefreshed { feed: id, .. } = events.recv().await.unwrap() {
                    return id;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(refreshed, feed.slug);
        cancel.cancel();
    }

    fn requests(env: &Env) -> usize {
        env.traffic.requests.load(Ordering::SeqCst)
    }

    /// The same database file opened separately, as the CLI or a second server would.
    async fn other_process(env: &Env) -> Db {
        Db::open(&env._dir.path().join("feedrsauros.db"))
            .await
            .unwrap()
    }

    async fn next_resync(events: &mut broadcast::Receiver<PollerEvent>) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while events.recv().await.unwrap() != PollerEvent::Resync {}
        })
        .await
        .expect("no resync event");
    }

    #[tokio::test]
    async fn pauses_while_inactive_and_catches_up_when_active_again() {
        let env = env().await;
        let (handle, cancel, _task) = start(&env).await;
        handle.set_active(false);
        env.add(env.base.join("feeds/a").unwrap(), Utc::now()).await;
        handle.wake();

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(requests(&env), 0);

        handle.set_active(true);
        eventually("the feed is fetched after resuming", async || {
            requests(&env) == 1
        })
        .await;
        cancel.cancel();
    }

    #[tokio::test]
    async fn only_one_poller_fetches_from_a_database() {
        let env = env().await;
        for name in ["a", "b", "c"] {
            env.add(env.base.join(&format!("slow/{name}")).unwrap(), Utc::now())
                .await;
        }
        let (_first, cancel, _task) = start(&env).await;
        let other = CancellationToken::new();
        let (_second, _second_task) = poller::spawn(
            other_process(&env).await,
            env.fetcher.clone(),
            env.jev.clone(),
            other.clone(),
        );

        eventually("every feed is fetched", async || requests(&env) >= 3).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(requests(&env), 3);
        cancel.cancel();
        other.cancel();
    }

    #[tokio::test]
    async fn takes_over_when_the_other_poller_stops() {
        let env = env().await;
        env.add(env.base.join("feeds/a").unwrap(), Utc::now()).await;
        let (_first, cancel, task) = start(&env).await;
        eventually("the first poller fetches", async || requests(&env) == 1).await;
        let other = CancellationToken::new();
        let (second, _second_task) = poller::spawn(
            other_process(&env).await,
            env.fetcher.clone(),
            env.jev.clone(),
            other.clone(),
        );

        cancel.cancel();
        task.await.unwrap();
        second.refresh(FeedScope::All).await.unwrap();

        eventually("the second poller fetches", async || requests(&env) == 2).await;
        other.cancel();
    }

    #[tokio::test]
    async fn reports_changes_made_by_another_process() {
        let env = env().await;
        let (handle, cancel, _task) = start(&env).await;
        let mut events = handle.subscribe();

        other_process(&env)
            .await
            .create_folder("From the CLI")
            .await
            .unwrap();

        next_resync(&mut events).await;
        cancel.cancel();
    }

    #[tokio::test]
    async fn does_not_report_its_own_changes() {
        let env = env().await;
        let (handle, cancel, _task) = start(&env).await;
        let mut events = handle.subscribe();

        env.db.create_folder("From the web app").await.unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;

        while let Ok(event) = events.try_recv() {
            assert_ne!(event, PollerEvent::Resync);
        }
        cancel.cancel();
    }

    #[tokio::test]
    async fn stops_when_cancelled() {
        let env = env().await;
        let (_handle, cancel, task) = start(&env).await;

        cancel.cancel();

        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
}

mod filters {
    use super::*;

    const ALL_POSTS: [&str; 4] = [
        "Another linkless note",
        "First post",
        "Linkless note",
        "Second post",
    ];

    #[tokio::test]
    async fn hide_new_articles_the_reader_does_not_want() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;
        env.turn_on_ai().await;
        env.hide_first_posts(feed.id).await;

        env.batch(vec![feed.clone()]).await;

        assert_eq!(
            env.titles(feed.id).await,
            ["Another linkless note", "Linkless note", "Second post"]
        );
        assert_eq!(env.jev_requests(), 4, "one request per new article");
    }

    #[tokio::test]
    async fn keep_only_what_the_reader_wants_to_see() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;
        env.turn_on_ai().await;
        let filters = Filters::new(Some("first posts"), None);
        env.db.set_filters(feed.id, &filters).await.unwrap();

        env.batch(vec![feed.clone()]).await;

        assert_eq!(env.titles(feed.id).await, ["First post"]);
    }

    #[tokio::test]
    async fn do_not_ask_again_about_articles_already_judged() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;
        env.turn_on_ai().await;
        env.hide_first_posts(feed.id).await;
        env.batch(vec![feed.clone()]).await;

        env.batch(vec![feed.clone()]).await;

        assert_eq!(env.jev_requests(), 4);
        assert!(
            !env.titles(feed.id)
                .await
                .contains(&"First post".to_string())
        );
    }

    #[tokio::test]
    async fn keep_every_article_while_ai_is_off() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;
        env.db.set_api_key("ts_test_key").await.unwrap();
        env.hide_first_posts(feed.id).await;

        env.batch(vec![feed.clone()]).await;

        assert_eq!(env.titles(feed.id).await, ALL_POSTS);
        assert_eq!(env.jev_requests(), 0);
    }

    #[tokio::test]
    async fn keep_articles_when_jev_fails() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;
        env.turn_on_ai().await;
        env.hide_first_posts(feed.id).await;
        env.traffic.jev_down.store(true, Ordering::SeqCst);

        env.batch(vec![feed.clone()]).await;

        assert_eq!(env.titles(feed.id).await, ALL_POSTS);
    }

    #[tokio::test]
    async fn do_not_ask_jev_about_feeds_without_filters() {
        let env = env().await;
        let feed = env.add_path("feeds/a").await;
        env.turn_on_ai().await;

        env.batch(vec![feed.clone()]).await;

        assert_eq!(env.titles(feed.id).await, ALL_POSTS);
        assert_eq!(env.jev_requests(), 0);
    }
}
