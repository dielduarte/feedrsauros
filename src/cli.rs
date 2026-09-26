use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use anyhow::Context;
use chrono::Utc;
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use tokio::sync::broadcast::{self, error::RecvError};

use crate::add_feed::{Placement, add_feed, parse_input};
use crate::db::{Db, SidebarFeed};
use crate::fetch::{DEFAULT_TIMEOUT, Fetcher};
use crate::jev::{self, Jev};
use crate::model::FeedScope;
use crate::opml;
use crate::poller::{BatchHealth, PollerEvent, run_batch};
use crate::server;

#[derive(Parser)]
#[command(name = "feedrsauros", version, about = "A local-first RSS reader")]
pub struct Cli {
    /// Database file [default: the platform data directory]
    #[arg(long, env = "FEEDRSAUROS_DB", global = true)]
    pub db: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the background poller and the web API
    Serve {
        /// Address to listen on. feedrsauros has no login, so only expose it beyond this machine
        /// behind something that authenticates, like a reverse proxy.
        #[arg(long, env = "FEEDRSAUROS_HOST", default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
        host: IpAddr,
        #[arg(long, env = "FEEDRSAUROS_PORT", default_value_t = 7777)]
        port: u16,
        /// Open the web app in your browser
        #[arg(long)]
        open: bool,
    },
    /// Subscribe to a feed, or to a site that links to one
    Add {
        url: String,
        /// Put the feed in this folder, creating it if needed
        #[arg(long)]
        folder: Option<String>,
    },
    /// Fetch every feed now
    Refresh,
    /// List folders and feeds with unread counts
    Ls,
    /// Import subscriptions from an OPML file
    Import { file: PathBuf },
    /// Print subscriptions as OPML
    Export,
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let path = database_path(cli.db)?;
    let db = Db::open(&path)
        .await
        .with_context(|| format!("could not open {}", path.display()))?;
    match cli.command {
        Command::Serve { host, port, open } => serve(db, host, port, open).await,
        Command::Add { url, folder } => add(db, &url, folder).await,
        Command::Refresh => refresh(db).await,
        Command::Ls => list(db).await,
        Command::Import { file } => import(db, file).await,
        Command::Export => {
            print!("{}", opml::render(&db.sidebar().await?));
            Ok(())
        }
    }
}

/// `explicit`, or the file in the platform's data directory that every feedrsauros program shares.
pub fn database_path(explicit: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let path = match explicit {
        Some(path) => path,
        None => ProjectDirs::from("", "", "feedrsauros")
            .context("could not find a data directory; pass --db")?
            .data_dir()
            .join("feedrsauros.db"),
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn plural(count: impl Into<u64>, word: &str) -> String {
    match count.into() {
        1 => format!("1 {word}"),
        n => format!("{n} {word}s"),
    }
}

async fn serve(db: Db, host: IpAddr, port: u16, open: bool) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind((host, port))
        .await
        .with_context(|| format!("could not listen on {host}:{port}"))?;
    let server = server::start(db, listener).await?;
    let url = format!("http://{}", server.addr);
    println!("feedrsauros is running at {url}");
    if open && let Err(error) = open::that_detached(&url) {
        tracing::warn!(%error, "could not open a browser");
    }
    shutdown_signal().await;
    server.shutdown().await
}

async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                terminate.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}

async fn add(db: Db, input: &str, folder: Option<String>) -> anyhow::Result<()> {
    let url = parse_input(input).with_context(|| format!("not a web address: {input}"))?;
    let jev = Jev::from_settings(&db, jev::ENDPOINT.parse()?).await?;
    let placement = match (folder, &jev) {
        (Some(name), _) => Placement::Folder(db.ensure_folder(&name).await?.id),
        (None, Some(jev)) => Placement::BestFit(jev),
        (None, None) => Placement::Unfiled,
    };
    let fetcher = Fetcher::new(DEFAULT_TIMEOUT);
    let added = add_feed(&db, &fetcher, &url, placement, Utc::now()).await?;
    let filed = match &added.ai_folder {
        Some(folder) => format!(", filed under {folder} by Jev"),
        None => String::new(),
    };
    println!(
        "Added {} ({}{filed})",
        added.title,
        plural(added.new_items, "new item")
    );
    Ok(())
}

async fn refresh(db: Db) -> anyhow::Result<()> {
    let now = Utc::now();
    db.mark_due(FeedScope::All, now).await?;
    let feeds = db.feeds_due(now).await?;
    if feeds.is_empty() {
        println!("No feeds yet. Add one with `feedrsauros add <url>`.");
        return Ok(());
    }
    let count = feeds.len() as u64;
    let titles: HashMap<_, _> = feeds
        .iter()
        .map(|f| {
            (
                f.slug.clone(),
                f.custom_title.clone().unwrap_or_else(|| f.title.clone()),
            )
        })
        .collect();

    let (events, mut received) = broadcast::channel(1024);
    let printer = tokio::spawn(async move {
        let (mut new_items, mut failed) = (0, 0u64);
        loop {
            match received.recv().await {
                Ok(PollerEvent::FeedRefreshed { feed, new_items: n }) if n > 0 => {
                    new_items += n;
                    println!("  +{n:<4} {}", titles[&feed]);
                }
                Ok(PollerEvent::FeedFailed { feed, error }) => {
                    failed += 1;
                    println!("  !     {}: {error}", titles[&feed]);
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return (new_items, failed),
            }
        }
    });

    let jev = Jev::from_settings(&db, jev::ENDPOINT.parse()?).await?;
    let health = run_batch(
        &db,
        &Fetcher::new(DEFAULT_TIMEOUT),
        jev.as_ref(),
        feeds,
        now,
        &events,
    )
    .await;
    drop(events);
    let (new_items, failed) = printer.await?;

    match health {
        BatchHealth::Offline => println!("You seem to be offline; nothing was changed."),
        BatchHealth::Online => {
            let failures = match failed {
                0 => String::new(),
                n => format!(", {} failed", plural(n, "feed")),
            };
            println!(
                "Checked {}: {}{failures}",
                plural(count, "feed"),
                plural(new_items, "new item")
            );
        }
    }
    Ok(())
}

async fn list(db: Db) -> anyhow::Result<()> {
    let sidebar = db.sidebar().await?;
    if sidebar.folders.is_empty() && sidebar.uncategorized.is_empty() {
        println!("No feeds yet. Add one with `feedrsauros add <url>`.");
        return Ok(());
    }
    for folder in &sidebar.folders {
        println!("{}", folder.folder.name);
        folder.feeds.iter().for_each(print_feed);
    }
    if !sidebar.uncategorized.is_empty() {
        println!("Uncategorized");
        sidebar.uncategorized.iter().for_each(print_feed);
    }
    println!("{} unread", sidebar.total_unread());
    Ok(())
}

fn print_feed(feed: &SidebarFeed) {
    match &feed.last_error {
        None => println!("{:>5}  {}", feed.unread, feed.title),
        Some(error) => println!("{:>5}  {}  (failing: {error})", feed.unread, feed.title),
    }
}

async fn import(db: Db, file: PathBuf) -> anyhow::Result<()> {
    let xml = std::fs::read_to_string(&file)
        .with_context(|| format!("could not read {}", file.display()))?;
    let report = opml::import(&db, opml::parse(&xml)?, Utc::now()).await?;
    println!(
        "Added {}, skipped {} already subscribed.",
        plural(report.added, "feed"),
        report.skipped
    );
    for url in &report.invalid {
        println!("  ignored invalid feed URL: {url}");
    }
    if report.added > 0 {
        println!("Run `feedrsauros refresh` or `feedrsauros serve` to fetch them.");
    }
    Ok(())
}
