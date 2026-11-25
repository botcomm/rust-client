use std::collections::HashMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::bot::Inner;
use crate::{Bot, Error, KeyboardOption, Message};

/// One field of the bot's per-session settings schema; values are strings
/// (`"true"`/`"false"` for `toggle`, `HH:MM` for `time`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingField {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `toggle`, `select`, `text` or `time`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<KeyboardOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// The effective settings of a session after a change, by either side.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionSettings {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub settings: HashMap<String, String>,
}

/// The account on the other side of a session, as bots see it.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionUser {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
}

/// A conversation between one account and the bot.
#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(rename = "unreadCount", default)]
    pub unread_count: i64,
    #[serde(rename = "lastMessage", default)]
    pub last_message: Option<Message>,
    #[serde(default)]
    pub user: Option<SessionUser>,
    /// The user's values for the bot's settings schema, defaults filled in.
    #[serde(default)]
    pub settings: HashMap<String, String>,
    /// A muted session receives messages but no push notifications.
    #[serde(default)]
    pub muted: bool,
    #[serde(rename = "createTS", with = "time::serde::rfc3339")]
    pub create_ts: OffsetDateTime,
}

impl Bot {
    /// Publishes the per-session settings schema; changes arrive as
    /// [`crate::Event::Settings`] and every [`Session`] carries the current values.
    pub async fn register_settings(&self, fields: &[SettingField]) -> Result<(), Error> {
        let body = serde_json::json!({ "settings": fields });
        self.inner
            .api(Method::POST, &format!("/bot/{}", self.inner.id), Some(body))
            .await?;
        Ok(())
    }

    /// Changes settings of a session on the user's behalf.
    pub async fn set_settings(
        &self,
        session_id: &str,
        values: HashMap<String, String>,
    ) -> Result<SessionSettings, Error> {
        self.inner
            .api_json(
                Method::POST,
                &format!("/session/{session_id}/settings"),
                Some(serde_json::json!(values)),
            )
            .await
    }

    /// Fetches all sessions of the bot.
    pub async fn sessions(&self) -> Result<Vec<Session>, Error> {
        self.inner.sessions().await
    }
}

impl Inner {
    pub(super) async fn sessions(&self) -> Result<Vec<Session>, Error> {
        self.api_json(Method::GET, "/session", None).await
    }
}
