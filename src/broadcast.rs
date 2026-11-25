use reqwest::Method;
use serde::Deserialize;
use time::OffsetDateTime;

use crate::{Bot, Error, Keyboard, KeyboardOption, SendOptions};

/// A broadcast job created by [`Bot::broadcast`].
#[derive(Debug, Clone, Deserialize)]
pub struct Broadcast {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub silent: bool,
    pub total: i64,
    pub sent: i64,
    pub failed: i64,
    pub done: bool,
    #[serde(rename = "createTS", with = "time::serde::rfc3339")]
    pub create_ts: OffsetDateTime,
    #[serde(rename = "doneTS", default, with = "time::serde::rfc3339::option")]
    pub done_ts: Option<OffsetDateTime>,
}

impl Bot {
    /// Sends one message to every session of the bot as a background job;
    /// poll [`Bot::broadcast_status`] for progress.
    pub async fn broadcast(
        &self,
        content: &str,
        keyboard: Option<Vec<KeyboardOption>>,
        silent: bool,
    ) -> Result<Broadcast, Error> {
        self.broadcast_with_options(
            content,
            SendOptions {
                keyboard: keyboard.map(|options| Keyboard {
                    options,
                    selected: None,
                }),
                silent,
                ..Default::default()
            },
        )
        .await
    }

    /// Includes an optional subject or keyboard on every broadcast copy.
    pub async fn broadcast_with_options(
        &self,
        content: &str,
        options: SendOptions,
    ) -> Result<Broadcast, Error> {
        let mut body = serde_json::to_value(options)?;
        body["content"] = content.into();
        self.inner
            .api_json(
                Method::POST,
                &format!("/bot/{}/broadcast", self.inner.id),
                Some(body),
            )
            .await
    }

    pub async fn broadcast_status(&self, id: &str) -> Result<Broadcast, Error> {
        self.inner
            .api_json(
                Method::GET,
                &format!("/bot/{}/broadcast/{id}", self.inner.id),
                None,
            )
            .await
    }
}
