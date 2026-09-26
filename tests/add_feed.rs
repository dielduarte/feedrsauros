use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use chrono::{DateTime, TimeZone, Utc};
use feedrsauros::add_feed::{AddFeedError, Placement, add_feed};
use feedrsauros::db::{Db, ItemQuery, ItemScope};
use feedrsauros::fetch::{FetchError, Fetcher};
use feedrsauros::schedule::POLL_INTERVAL;
use tempfile::TempDir;
use url::Url;

struct Env {
    db: Db,
    fetcher: Fetcher,
    base: Url,
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

async fn env(app: Router) -> Env {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("feedrsauros.db")).await.unwrap();
    Env {
        db,
        fetcher: Fetcher::new(Duration::from_secs(5)),
        base,
        _dir: dir,
    }
}

fn site_with_link_tag() -> Router {
    Router::new()
        .route(
            "/",
            get(|| async {
                Html(r#"<html><head><link rel="alternate" type="application/rss+xml" href="/posts/rss"></head></html>"#)
            }),
        )
        .route("/posts/rss", get(|| async { rss2() }))
}

impl Env {
    fn url(&self, path: &str) -> Url {
        self.base.join(path).unwrap()
    }

    async fn add(&self, path: &str) -> Result<feedrsauros::add_feed::Added, AddFeedError> {
        add_feed(
            &self.db,
            &self.fetcher,
            &self.url(path),
            Placement::Unfiled,
            now(),
        )
        .await
    }
}

#[tokio::test]
async fn subscribes_to_a_feed_url_with_its_items() {
    let env = env(Router::new().route("/feed.xml", get(|| async { rss2() }))).await;

    let added = env.add("feed.xml").await.unwrap();

    assert_eq!(added.title, "Example Blog");
    assert_eq!(added.new_items, 4);
    let sidebar = env.db.sidebar().await.unwrap();
    assert_eq!(sidebar.uncategorized[0].id, added.id);
    assert_eq!(sidebar.uncategorized[0].unread, 4);
}

#[tokio::test]
async fn schedules_the_next_poll_one_interval_later() {
    let env = env(Router::new().route("/feed.xml", get(|| async { rss2() }))).await;

    env.add("feed.xml").await.unwrap();

    assert!(env.db.feeds_due(now()).await.unwrap().is_empty());
    let due = env.db.feeds_due(now() + POLL_INTERVAL).await.unwrap();
    assert_eq!(due[0].url, env.url("feed.xml"));
}

#[tokio::test]
async fn discovers_the_feed_from_a_site_link_tag() {
    let env = env(site_with_link_tag()).await;

    env.add("").await.unwrap();

    let feed = &env.db.feeds_due(now() + POLL_INTERVAL).await.unwrap()[0];
    assert_eq!(feed.url, env.url("posts/rss"));
    assert_eq!(
        feed.site_url.as_ref().unwrap().as_str(),
        "https://example.com/"
    );
}

#[tokio::test]
async fn falls_back_to_common_feed_paths() {
    let app = Router::new()
        .route(
            "/",
            get(|| async { Html("<html><head><title>No links</title></head></html>") }),
        )
        .route("/atom.xml", get(|| async { rss2() }));
    let env = env(app).await;

    env.add("").await.unwrap();

    let feed = &env.db.feeds_due(now() + POLL_INTERVAL).await.unwrap()[0];
    assert_eq!(feed.url, env.url("atom.xml"));
}

#[tokio::test]
async fn reports_when_a_site_has_no_feed() {
    let env = env(Router::new().route("/", get(|| async { Html("<html></html>") }))).await;

    let result = env.add("").await;

    assert!(matches!(result, Err(AddFeedError::NoFeedFound)));
    assert!(env.db.sidebar().await.unwrap().uncategorized.is_empty());
}

#[tokio::test]
async fn rejects_a_feed_already_subscribed_through_its_site() {
    let env = env(site_with_link_tag()).await;
    env.add("posts/rss").await.unwrap();

    let result = env.add("").await;

    assert!(matches!(result, Err(AddFeedError::AlreadySubscribed)));
}

#[tokio::test]
async fn can_subscribe_into_a_folder() {
    let env = env(Router::new().route("/feed.xml", get(|| async { rss2() }))).await;
    let folder = env.db.create_folder("Tech").await.unwrap();

    let added = add_feed(
        &env.db,
        &env.fetcher,
        &env.url("feed.xml"),
        Placement::Folder(folder.id),
        now(),
    )
    .await
    .unwrap();

    let sidebar = env.db.sidebar().await.unwrap();
    assert_eq!(sidebar.folders[0].feeds[0].id, added.id);
}

#[tokio::test]
async fn stores_sanitized_content() {
    let env = env(Router::new().route("/feed.xml", get(|| async { rss2() }))).await;
    env.add("feed.xml").await.unwrap();
    let query = ItemQuery {
        scope: ItemScope::All,
        unread_only: false,
        cursor: None,
        limit: 10,
    };
    let first = env.db.list_items(query).await.unwrap().items;
    let post = first
        .iter()
        .find(|i| i.title.as_deref() == Some("First post"))
        .unwrap();

    let content = env.db.get_item(post.id).await.unwrap().content.unwrap();

    assert!(!content.as_str().contains("script"));
}

#[tokio::test]
async fn reports_an_unreachable_url() {
    let env = env(Router::new().route("/gone", get(|| async { StatusCode::NOT_FOUND }))).await;

    let result = env.add("gone").await;

    assert!(matches!(
        result,
        Err(AddFeedError::Fetch(FetchError::Http(404)))
    ));
}

mod input {
    use feedrsauros::add_feed::parse_input;

    #[test]
    fn accepts_full_urls() {
        assert_eq!(
            parse_input("http://blog.example.com/feed")
                .unwrap()
                .as_str(),
            "http://blog.example.com/feed"
        );
    }

    #[test]
    fn assumes_https_for_bare_domains() {
        assert_eq!(
            parse_input("  example.com/blog ").unwrap().as_str(),
            "https://example.com/blog"
        );
    }

    #[test]
    fn rejects_anything_that_is_not_a_web_address() {
        assert!(parse_input("not a url").is_none());
        assert!(parse_input("ftp://example.com/feed").is_none());
        assert!(parse_input("javascript:alert(1)").is_none());
        assert!(parse_input("").is_none());
    }
}
