use std::collections::HashMap;

use serde::Deserialize;

use crate::{Bot, Error, Message};

/// Compatibility wrapper. New integrations use Message directly.
#[derive(Debug, Clone, Deserialize)]
pub struct CardMessage {
    #[serde(flatten)]
    pub message: Message,
    #[serde(default)]
    pub card: Option<serde_json::Value>,
    #[serde(default)]
    pub fields: HashMap<String, serde_json::Value>,
}

impl From<Message> for CardMessage {
    fn from(message: Message) -> Self {
        Self {
            card: message.card.clone(),
            fields: message.fields.clone(),
            message,
        }
    }
}

impl Bot {
    pub async fn send_card(
        &self,
        session_id: &str,
        card: serde_json::Value,
    ) -> Result<CardMessage, Error> {
        Ok(self
            .send_message(session_id, serde_json::json!({ "card": card }))
            .await?
            .into())
    }

    pub async fn update_card(
        &self,
        session_id: &str,
        message_id: &str,
        change: serde_json::Value,
    ) -> Result<CardMessage, Error> {
        Ok(self
            .update_message(
                session_id,
                message_id,
                serde_json::json!({ "card": change }),
            )
            .await?
            .into())
    }

    pub async fn get_card_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<CardMessage, Error> {
        Ok(self.get_message(session_id, message_id).await?.into())
    }
}
