use super::{Db, DbError, found};
use crate::secrets::Vault;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub ai_enabled: bool,
    /// The last characters of the saved TypeSafe API key, if there is one.
    pub api_key_hint: Option<String>,
}

const HINT_LEN: usize = 4;

impl Db {
    pub async fn settings(&self) -> Result<Settings, DbError> {
        let row = sqlx::query!(
            r#"SELECT ai_enabled AS "ai_enabled: bool", typesafe_api_key_hint FROM settings"#
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(Settings {
            ai_enabled: row.ai_enabled,
            api_key_hint: row.typesafe_api_key_hint,
        })
    }

    /// Stores the key encrypted; only its last few characters are kept readable, as a hint.
    pub async fn set_api_key(&self, key: &str) -> Result<(), DbError> {
        let sealed = Vault::beside(self.path())?.seal(key);
        let hint: String = key
            .chars()
            .rev()
            .take(HINT_LEN)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        sqlx::query!(
            "UPDATE settings SET typesafe_api_key = ?, typesafe_api_key_hint = ?",
            sealed,
            hint
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// AI features need the key, so they're turned off with it.
    pub async fn clear_api_key(&self) -> Result<(), DbError> {
        sqlx::query!(
            "UPDATE settings SET typesafe_api_key = NULL, typesafe_api_key_hint = NULL, ai_enabled = 0"
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Fails with `NotFound` when turning AI on without a saved key.
    pub async fn set_ai_enabled(&self, enabled: bool) -> Result<(), DbError> {
        found(
            sqlx::query!(
                "UPDATE settings SET ai_enabled = ?1 WHERE ?1 = 0 OR typesafe_api_key IS NOT NULL",
                enabled
            )
            .execute(&self.pool)
            .await?,
        )
    }

    /// The decrypted API key, but only while AI features are turned on.
    pub async fn ai_api_key(&self) -> Result<Option<String>, DbError> {
        let sealed =
            sqlx::query_scalar!("SELECT typesafe_api_key FROM settings WHERE ai_enabled = 1")
                .fetch_optional(&self.pool)
                .await?
                .flatten();
        match sealed {
            Some(sealed) => Ok(Some(Vault::beside(self.path())?.open(&sealed)?)),
            None => Ok(None),
        }
    }
}
