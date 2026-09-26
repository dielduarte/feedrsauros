use std::net::SocketAddr;

use anyhow::Context;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::{self, AppState};
use crate::db::Db;
use crate::fetch::{DEFAULT_TIMEOUT, Fetcher};
use crate::jev;
use crate::poller::{self, PollerHandle};

/// The web API and poller, running in the background. Shared by `feedrsauros serve` and the
/// desktop app.
pub struct Server {
    pub addr: SocketAddr,
    pub poller: PollerHandle,
    cancel: CancellationToken,
    task: JoinHandle<anyhow::Result<()>>,
}

pub async fn start(db: Db, listener: TcpListener) -> anyhow::Result<Server> {
    let addr = listener
        .local_addr()
        .context("could not read the listening address")?;
    let fetcher = Fetcher::new(DEFAULT_TIMEOUT);
    let cancel = CancellationToken::new();
    let (poller, poller_task) = poller::spawn(db.clone(), fetcher.clone(), cancel.clone());
    let app = api::router(AppState {
        db,
        fetcher,
        poller: poller.clone(),
        typesafe: jev::ENDPOINT
            .parse()
            .expect("the TypeSafe endpoint is a valid URL"),
    });
    let stopping = cancel.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stopping.cancelled_owned())
            .await?;
        poller_task.await?;
        Ok(())
    });
    Ok(Server {
        addr,
        poller,
        cancel,
        task,
    })
}

impl Server {
    /// Stops the poller, which also ends open event streams, then waits for both to finish.
    pub async fn shutdown(self) -> anyhow::Result<()> {
        self.cancel.cancel();
        self.task.await?
    }
}
