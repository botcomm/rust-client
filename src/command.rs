use std::sync::atomic::Ordering;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::bot::Inner;
use crate::{Bot, Error, Event, Message};

/// A single text argument collected by the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandInput {
    #[serde(default = "text_input_type", rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, rename = "maxLength")]
    pub max_length: usize,
}

fn text_input_type() -> String {
    "text".into()
}

impl Default for CommandInput {
    fn default() -> Self {
        Self {
            kind: text_input_type(),
            label: String::new(),
            placeholder: String::new(),
            required: false,
            max_length: 256,
        }
    }
}

/// A bot command shown to users.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<CommandInput>,
    pub name: String,
    pub value: String,
    pub description: String,
}

impl Bot {
    /// Publishes the bot commands to the API and routes incoming messages
    /// whose content equals a command value to [`Event::Command`].
    pub async fn register_commands(&self, commands: &[Command]) -> Result<(), Error> {
        let body = serde_json::json!({ "commands": commands });
        self.inner
            .api(Method::POST, &format!("/bot/{}", self.inner.id), Some(body))
            .await?;
        *self.inner.commands.write().await = commands
            .iter()
            .map(|c| (c.value.clone(), c.input.is_some()))
            .collect();
        Ok(())
    }

    /// Opt in to argument routing. Content remains unchanged on received messages.
    pub fn set_argument_commands(&self, enabled: bool) {
        self.inner
            .argument_commands
            .store(enabled, Ordering::Relaxed);
    }

    /// Configure user input after successfully publishing commands.
    pub async fn set_input_mode(&self, mode: &str) -> Result<(), Error> {
        self.inner
            .api(
                Method::POST,
                &format!("/bot/{}", self.inner.id),
                Some(serde_json::json!({ "inputMode": mode })),
            )
            .await?;
        Ok(())
    }
}

impl Inner {
    /// Delivers a message as [`Event::Command`] when its content (or, for
    /// keyboard answers, its machine value) matches a registered command
    /// value, as [`Event::Message`] otherwise.
    pub(super) async fn route(&self, mut msg: Message, tx: &mpsc::Sender<Event>) -> bool {
        let key = msg
            .value
            .as_deref()
            .filter(|v| !v.is_empty())
            .unwrap_or(&msg.content);
        let commands = self.commands.read().await;
        let mut is_command = commands.contains_key(key);
        if self.argument_commands.load(Ordering::Relaxed)
            && msg.value.as_deref().unwrap_or("").is_empty()
            && msg.reply_to.as_deref().unwrap_or("").is_empty()
        {
            if is_command {
                msg.command = Some(key.to_string());
                msg.argument = Some(String::new());
            } else {
                let command = msg
                    .content
                    .split([' ', '\t', '\r', '\n'])
                    .next()
                    .unwrap_or("");
                if commands.get(command).copied().unwrap_or(false) {
                    is_command = true;
                    msg.command = Some(command.to_string());
                    msg.argument = Some(
                        msg.content[command.len()..]
                            .trim_matches([' ', '\t', '\r', '\n'])
                            .to_string(),
                    );
                }
            }
        }
        drop(commands);
        let event = if is_command {
            Event::Command(msg)
        } else {
            Event::Message(msg)
        };
        tx.send(event).await.is_ok()
    }
}
