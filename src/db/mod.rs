mod feeds;
mod folders;
mod items;
mod polling;
mod settings;
mod sidebar;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteQueryResult};

pub use feeds::{Feed, FetchRecord, NewFeed};
pub use folders::Folder;
pub use items::{Cursor, Item, ItemQuery, ItemScope, ItemSummary, Page};
pub use settings::Settings;
pub use sidebar::{Sidebar, SidebarFeed, SidebarFolder};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("not found")]
    NotFound,
    #[error("already exists")]
    AlreadyExists,
    #[error(transparent)]
    Sqlx(sqlx::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Secret(#[from] crate::secrets::VaultError),
}

impl From<sqlx::Error> for DbError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::RowNotFound => Self::NotFound,
            sqlx::Error::Database(e) if e.is_unique_violation() => Self::AlreadyExists,
            sqlx::Error::Database(e) if e.is_foreign_key_violation() => Self::NotFound,
            _ => Self::Sqlx(error),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
    path: Arc<PathBuf>,
}

impl Db {
    pub async fn open(path: &Path) -> Result<Self, DbError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        // One connection that is never recycled: SQLite's `data_version` then changes only when
        // another process writes, which is how the server notices changes made by the CLI.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Self {
            pool,
            path: Arc::new(path.to_path_buf()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Changes whenever another process commits to the database; this process's writes don't.
    pub async fn data_version(&self) -> Result<i64, DbError> {
        Ok(sqlx::query_scalar("PRAGMA data_version")
            .fetch_one(&self.pool)
            .await?)
    }
}

fn ts(time: DateTime<Utc>) -> i64 {
    time.timestamp()
}

fn from_ts(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap_or_default()
}

fn found(result: SqliteQueryResult) -> Result<(), DbError> {
    match result.rows_affected() {
        0 => Err(DbError::NotFound),
        _ => Ok(()),
    }
}

/// Moves `id` to `index` within `siblings` (clamped to the end), returning the new order.
fn place<T: PartialEq>(mut siblings: Vec<T>, id: T, index: usize) -> Vec<T> {
    siblings.retain(|s| *s != id);
    siblings.insert(index.min(siblings.len()), id);
    siblings
}
