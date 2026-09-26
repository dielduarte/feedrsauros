use std::collections::HashSet;

use super::{Db, DbError};
use crate::filter::Filters;
use crate::model::FeedId;

impl Db {
    pub async fn filters(&self, feed: FeedId) -> Result<Filters, DbError> {
        let row = sqlx::query!(
            "SELECT filter_wanted, filter_unwanted FROM feeds WHERE id = ?",
            feed
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(Filters {
            wanted: row.filter_wanted,
            unwanted: row.filter_unwanted,
        })
    }

    pub async fn set_filters(&self, feed: FeedId, filters: &Filters) -> Result<(), DbError> {
        super::found(
            sqlx::query!(
                "UPDATE feeds SET filter_wanted = ?, filter_unwanted = ? WHERE id = ?",
                filters.wanted,
                filters.unwanted,
                feed
            )
            .execute(&self.pool)
            .await?,
        )
    }

    /// Guids of the feed's stored articles, hidden ones included: each is judged once on arrival.
    pub async fn known_guids(&self, feed: FeedId) -> Result<HashSet<String>, DbError> {
        Ok(
            sqlx::query_scalar!("SELECT guid FROM items WHERE feed_id = ?", feed)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .collect(),
        )
    }
}

/// A stored article, as much of it as a rule needs.
pub struct SavedArticle {
    pub guid: String,
    pub title: Option<String>,
    pub summary: Option<String>,
}

impl Db {
    /// The feed's own title and every stored article the filters may judge: hidden ones too, but
    /// not starred ones, which stay whatever the filters say.
    pub async fn judgeable_articles(
        &self,
        feed: FeedId,
    ) -> Result<(String, Vec<SavedArticle>), DbError> {
        let site = sqlx::query_scalar!("SELECT title FROM feeds WHERE id = ?", feed)
            .fetch_one(&self.pool)
            .await?;
        let articles = sqlx::query_as!(
            SavedArticle,
            "SELECT guid, title, summary FROM items WHERE feed_id = ? AND starred_at IS NULL",
            feed
        )
        .fetch_all(&self.pool)
        .await?;
        Ok((site, articles))
    }

    /// Hides `hide` and brings back `show`. Returns how many of each actually changed.
    pub async fn set_hidden(
        &self,
        feed: FeedId,
        hide: &[String],
        show: &[String],
    ) -> Result<(u64, u64), DbError> {
        let mut tx = self.pool.begin().await?;
        let (mut hidden, mut shown) = (0, 0);
        for guid in hide {
            hidden += sqlx::query!(
                "UPDATE items SET hidden_at = unixepoch()
                 WHERE feed_id = ? AND guid = ? AND hidden_at IS NULL AND starred_at IS NULL",
                feed,
                guid
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        for guid in show {
            shown += sqlx::query!(
                "UPDATE items SET hidden_at = NULL WHERE feed_id = ? AND guid = ? AND hidden_at IS NOT NULL",
                feed,
                guid
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        tx.commit().await?;
        Ok((hidden, shown))
    }

    /// Brings back every hidden article, e.g. once a feed has no filters left.
    pub async fn show_all(&self, feed: FeedId) -> Result<u64, DbError> {
        Ok(sqlx::query!(
            "UPDATE items SET hidden_at = NULL WHERE feed_id = ? AND hidden_at IS NOT NULL",
            feed
        )
        .execute(&self.pool)
        .await?
        .rows_affected())
    }
}
