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

    /// Replaces the feed's rules with `rules`, in that order.
    pub async fn set_rules(&self, feed: FeedId, rules: &[Rule]) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!("DELETE FROM feed_rules WHERE feed_id = ?", feed)
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
