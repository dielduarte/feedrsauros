use std::collections::HashSet;

use super::{Db, DbError};
use crate::model::FeedId;
use crate::rules::{Rule, RuleAction};

impl Db {
    pub async fn rules(&self, feed: FeedId) -> Result<Vec<Rule>, DbError> {
        let rows = sqlx::query!(
            "SELECT condition, action FROM feed_rules WHERE feed_id = ? ORDER BY position",
            feed
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| Rule {
                condition: row.condition,
                action: match row.action.as_str() {
                    "keep_only" => RuleAction::KeepOnly,
                    _ => RuleAction::Hide,
                },
            })
            .collect())
    }

    /// Replaces the feed's rules with `rules`, in that order. What earlier rules kept out is
    /// forgotten, so the next fetch judges those articles again under the new rules.
    pub async fn set_rules(&self, feed: FeedId, rules: &[Rule]) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("DELETE FROM feed_rules WHERE feed_id = ?", feed)
            .execute(&mut *tx)
            .await?;
        sqlx::query!("DELETE FROM filtered_items WHERE feed_id = ?", feed)
            .execute(&mut *tx)
            .await?;
        for (position, rule) in rules.iter().enumerate() {
            let position = position as i64;
            let action = match rule.action {
                RuleAction::Hide => "hide",
                RuleAction::KeepOnly => "keep_only",
            };
            sqlx::query!(
                "INSERT INTO feed_rules (feed_id, position, condition, action) VALUES (?, ?, ?, ?)",
                feed,
                position,
                rule.condition,
                action
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Guids of articles already stored or already kept out by a rule.
    pub async fn known_guids(&self, feed: FeedId) -> Result<HashSet<String>, DbError> {
        Ok(sqlx::query_scalar!(
            "SELECT guid FROM items WHERE feed_id = ?1 UNION SELECT guid FROM filtered_items WHERE feed_id = ?1",
            feed
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .collect())
    }

    pub async fn filtered_guids(&self, feed: FeedId) -> Result<HashSet<String>, DbError> {
        Ok(
            sqlx::query_scalar!("SELECT guid FROM filtered_items WHERE feed_id = ?", feed)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .collect(),
        )
    }

    pub async fn remember_filtered(&self, feed: FeedId, guids: &[&str]) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for guid in guids {
            sqlx::query!(
                "INSERT OR IGNORE INTO filtered_items (feed_id, guid) VALUES (?, ?)",
                feed,
                guid
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

/// A stored article, as much of it as a rule needs.
pub struct SavedArticle {
    pub guid: String,
    pub title: Option<String>,
    pub summary: Option<String>,
}

impl Db {
    /// The feed's own title and its stored articles, except starred ones, which stay whatever
    /// the rules say.
    pub async fn unstarred_articles(
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

    /// Takes stored articles out of the list and remembers them as kept out.
    pub async fn hide_articles(&self, feed: FeedId, guids: &[String]) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for guid in guids {
            sqlx::query!(
                "DELETE FROM items WHERE feed_id = ? AND guid = ? AND starred_at IS NULL",
                feed,
                guid
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!(
                "INSERT OR IGNORE INTO filtered_items (feed_id, guid) VALUES (?, ?)",
                feed,
                guid
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
