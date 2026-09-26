use std::time::Duration;

use axum::Router;
use axum::routing::get;
use chrono::Utc;
use feedrsauros::api::{self, AppState};
use feedrsauros::db::{Db, NewFeed};
use feedrsauros::fetch::Fetcher;
use feedrsauros::model::FeedScope;
use feedrsauros::poller;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

fn rss2() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/rss2.xml"
    ))
    .unwrap()
}

async fn listen(app: Router) -> Url {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

/// Reads `data:` lines from the stream until one satisfies `wanted`.
async fn next_event(response: &mut reqwest::Response, wanted: impl Fn(&Value) -> bool) -> Value {
    let mut buffer = String::new();
    loop {
        let chunk = response.chunk().await.unwrap().expect("stream ended");
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buffer.find('\n') {
            let line: String = buffer.drain(..=end).collect();
            if let Some(data) = line.trim().strip_prefix("data:") {
                let event: Value = serde_json::from_str(data.trim()).unwrap();
                if wanted(&event) {
                    return event;
                }
            }
        }
    }
}

#[tokio::test]
async fn streams_poller_events_to_the_browser() {
    let feeds = listen(Router::new().route("/feed.xml", get(|| async { rss2() }))).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("feedrsauros.db")).await.unwrap();
    let cancel = CancellationToken::new();
    let fetcher = Fetcher::new(Duration::from_secs(5));
    let (poller, _task) = poller::spawn(db.clone(), fetcher.clone(), cancel.clone());
    let app = listen(api::router(AppState {
        db: db.clone(),
        fetcher,
        poller: poller.clone(),
        typesafe: "http://127.0.0.1:9/".parse().unwrap(),
    }))
    .await;

    let mut stream = reqwest::get(app.join("api/events").unwrap()).await.unwrap();
    assert_eq!(stream.headers()["content-type"], "text/event-stream");
    let new = NewFeed {
        url: feeds.join("feed.xml").unwrap(),
        title: "Example".into(),
        site_url: None,
        folder: None,
    };
    let feed = db.insert_feed(new, Utc::now()).await.unwrap();
    poller.refresh(FeedScope::All).await.unwrap();

    let event = tokio::time::timeout(
        Duration::from_secs(5),
        next_event(&mut stream, |e| e["type"] == "feed_refreshed"),
    )
    .await
    .unwrap();

    assert_eq!(event["feed"], feed.slug);
    assert_eq!(event["new_items"], 4);
    cancel.cancel();
}
