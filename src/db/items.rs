use chrono::{DateTime, Utc};
use sqlx::{QueryBuilder, Sqlite};
use url::Url;

use super::feeds::parse_optional_url;
use super::{Db, DbError, found, from_ts, ts};
use crate::model::{FeedId, FolderId, ItemId};
use crate::sanitize::SanitizedHtml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemScope {
    All,
    Folder(FolderId),
    Feed(FeedId),
    Starred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub published_at: DateTime<Utc>,
    pub id: ItemId,
}

#[derive(Debug, Clone)]
pub struct ItemQuery {
    pub scope: ItemScope,
    pub unread_only: bool,
    pub cursor: Option<Cursor>,
    pub limit: u32,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub items: Vec<ItemSummary>,
    pub next: Option<Cursor>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ItemSummary {
    #[serde(skip)]
    pub id: ItemId,
    #[serde(skip)]
    pub feed_id: FeedId,
    pub slug: String,
    pub feed_slug: String,
    pub feed_title: String,
    pub url: Option<Url>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub published_at: DateTime<Utc>,
    /// When feedrsauros stored it; "mark all read" uses this to spare articles that arrived later.
    pub fetched_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub starred_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Item {
    #[serde(flatten)]
    pub summary: ItemSummary,
    #[serde(rename = "content_html")]
    pub content: Option<SanitizedHtml>,
}

#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: ItemId,
    feed_id: FeedId,
    slug: String,
    feed_slug: String,
    feed_title: String,
    url: Option<String>,
    title: Option<String>,
    author: Option<String>,
    summary: Option<String>,
    published_at: i64,
    fetched_at: i64,
    read_at: Option<i64>,
    starred_at: Option<i64>,
}

impl From<SummaryRow> for ItemSummary {
    fn from(row: SummaryRow) -> Self {
        Self {
            id: row.id,
            feed_id: row.feed_id,
            slug: row.slug,
            feed_slug: row.feed_slug,
            feed_title: row.feed_title,
            url: parse_optional_url(row.url),
            title: row.title,
            author: row.author,
            summary: row.summary,
            published_at: from_ts(row.published_at),
            fetched_at: from_ts(row.fetched_at),
            read_at: row.read_at.map(from_ts),
            starred_at: row.starred_at.map(from_ts),
        }
    }
}

/// Written against the unaliased `items` table so it works in both SELECT and UPDATE.
fn push_scope(query: &mut QueryBuilder<Sqlite>, scope: ItemScope) {
    match scope {
        ItemScope::All => {}
        ItemScope::Feed(id) => {
            query.push(" AND items.feed_id = ").push_bind(id);
        }
        ItemScope::Folder(id) => {
            query
                .push(" AND items.feed_id IN (SELECT id FROM feeds WHERE folder_id = ")
                .push_bind(id)
                .push(")");
        }
        ItemScope::Starred => {
            query.push(" AND items.starred_at IS NOT NULL");
        }
    }
}

impl Db {
    pub async fn list_items(&self, query: ItemQuery) -> Result<Page, DbError> {
        let limit = query.limit as usize;
        let mut sql = QueryBuilder::new(
            "SELECT items.id, items.feed_id, items.slug, feeds.slug AS feed_slug,
                    COALESCE(feeds.custom_title, feeds.title) AS feed_title,
                    items.url, items.title, items.author, items.summary, items.published_at, items.fetched_at,
                    items.read_at, items.starred_at
             FROM items JOIN feeds ON feeds.id = items.feed_id
             WHERE items.hidden_at IS NULL",
        );
        push_scope(&mut sql, query.scope);
        if query.unread_only {
            sql.push(" AND items.read_at IS NULL");
        }
        if let Some(cursor) = query.cursor {
            sql.push(" AND (items.published_at, items.id) < (")
                .push_bind(ts(cursor.published_at))
                .push(", ")
                .push_bind(cursor.id)
                .push(")");
        }
        // One extra row tells us whether another page exists.
        sql.push(" ORDER BY items.published_at DESC, items.id DESC LIMIT ")
            .push_bind(limit as i64 + 1);

        let mut items: Vec<ItemSummary> = sql
            .build_query_as::<SummaryRow>()
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(ItemSummary::from)
            .collect();
        let next = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|i| Cursor {
                published_at: i.published_at,
                id: i.id,
            })
        } else {
            None
        };
        Ok(Page { items, next })
    }

    pub async fn get_item(&self, id: ItemId) -> Result<Item, DbError> {
        let row = sqlx::query!(
            r#"SELECT items.id AS "id: ItemId", items.feed_id AS "feed_id: FeedId", items.slug,
                      feeds.slug AS feed_slug, COALESCE(feeds.custom_title, feeds.title) AS "feed_title!: String",
                      items.url, items.title, items.author, items.summary, items.published_at, items.fetched_at,
                      items.read_at, items.starred_at,
                      items.content_html
               FROM items JOIN feeds ON feeds.id = items.feed_id
               WHERE items.id = ?"#,
            id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(Item {
            summary: SummaryRow {
                id: row.id,
                feed_id: row.feed_id,
                slug: row.slug,
                feed_slug: row.feed_slug,
                feed_title: row.feed_title,
                url: row.url,
                title: row.title,
                author: row.author,
                summary: row.summary,
                published_at: row.published_at,
                fetched_at: row.fetched_at,
                read_at: row.read_at,
                starred_at: row.starred_at,
            }
            .into(),
            content: row.content_html.map(SanitizedHtml::from_stored),
        })
    }

    pub async fn set_read(&self, id: ItemId, read: bool) -> Result<(), DbError> {
        found(
            sqlx::query!(
                "UPDATE items SET read_at = CASE WHEN ? THEN COALESCE(read_at, unixepoch()) END WHERE id = ?",
                read,
                id
            )
            .execute(&self.pool)
            .await?,
        )
    }

    pub async fn set_starred(&self, id: ItemId, starred: bool) -> Result<(), DbError> {
        found(
            sqlx::query!(
                "UPDATE items SET starred_at = CASE WHEN ? THEN COALESCE(starred_at, unixepoch()) END WHERE id = ?",
                starred,
                id
            )
            .execute(&self.pool)
            .await?,
        )
    }

    pub async fn item_id(&self, feed_slug: &str, slug: &str) -> Result<ItemId, DbError> {
        sqlx::query_scalar!(
            r#"SELECT items.id AS "id: ItemId" FROM items JOIN feeds ON feeds.id = items.feed_id
               WHERE feeds.slug = ? AND items.slug = ?"#,
            feed_slug,
            slug
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(DbError::NotFound)
    }

    /// Marks read what was already stored when the reader looked (`seen_until`, the newest
    /// `fetched_at` they had), sparing articles that arrived since, even older-dated ones.
    pub async fn mark_read(
        &self,
        scope: ItemScope,
        seen_until: DateTime<Utc>,
    ) -> Result<u64, DbError> {
        let mut sql = QueryBuilder::new(
            // Hidden articles stay unread, so they come back unread if the filters let them through.
            "UPDATE items SET read_at = unixepoch() WHERE items.read_at IS NULL AND items.hidden_at IS NULL AND items.fetched_at <= ",
        );
        sql.push_bind(ts(seen_until));
        push_scope(&mut sql, scope);
        Ok(sql.build().execute(&self.pool).await?.rows_affected())
    }
}
