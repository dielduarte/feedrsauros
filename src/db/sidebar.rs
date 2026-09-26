use url::Url;

use super::feeds::{parse_optional_url, parse_stored_url};
use super::{Db, DbError, Folder};
use crate::model::{FeedId, FolderId};

#[derive(Debug, Clone)]
pub struct Sidebar {
    pub folders: Vec<SidebarFolder>,
    pub uncategorized: Vec<SidebarFeed>,
    pub starred: u32,
}

#[derive(Debug, Clone)]
pub struct SidebarFolder {
    pub folder: Folder,
    pub feeds: Vec<SidebarFeed>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SidebarFeed {
    #[serde(skip)]
    pub id: FeedId,
    pub slug: String,
    pub title: String,
    pub url: Url,
    pub site_url: Option<Url>,
    pub unread: u32,
    pub last_error: Option<String>,
}

impl Sidebar {
    pub fn total_unread(&self) -> u32 {
        self.folders.iter().map(SidebarFolder::unread).sum::<u32>()
            + self.uncategorized.iter().map(|f| f.unread).sum::<u32>()
    }
}

impl SidebarFolder {
    pub fn unread(&self) -> u32 {
        self.feeds.iter().map(|f| f.unread).sum()
    }
}

impl Db {
    pub async fn sidebar(&self) -> Result<Sidebar, DbError> {
        let rows = sqlx::query!(
            r#"SELECT id AS "id: FeedId", slug, folder_id AS "folder_id: FolderId",
                      COALESCE(custom_title, title) AS "title!: String", url, site_url, last_error,
                      (SELECT COUNT(*) FROM items WHERE items.feed_id = feeds.id AND read_at IS NULL AND hidden_at IS NULL) AS "unread!: u32"
               FROM feeds
               ORDER BY position, id"#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut folders: Vec<SidebarFolder> = self
            .folders()
            .await?
            .into_iter()
            .map(|folder| SidebarFolder {
                folder,
                feeds: Vec::new(),
            })
            .collect();
        let mut uncategorized = Vec::new();

        for row in rows {
            let feed = SidebarFeed {
                id: row.id,
                slug: row.slug,
                title: row.title,
                url: parse_stored_url(&row.url)?,
                site_url: parse_optional_url(row.site_url),
                unread: row.unread,
                last_error: row.last_error,
            };
            match row.folder_id {
                Some(folder_id) => {
                    if let Some(folder) = folders.iter_mut().find(|f| f.folder.id == folder_id) {
                        folder.feeds.push(feed);
                    }
                }
                None => uncategorized.push(feed),
            }
        }

        let starred = sqlx::query_scalar!(
            r#"SELECT COUNT(*) AS "count!: u32" FROM items WHERE starred_at IS NOT NULL"#
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(Sidebar {
            folders,
            uncategorized,
            starred,
        })
    }
}
