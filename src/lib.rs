//! Rust SDK for building BotComm bots.
//!
//! A bot authenticates with its client credentials (`client_credentials`
//! grant, re-granted automatically before the access token expires), keeps a
//! Server-Sent Events stream from `/updates` and receives incoming [`Event`]s on the
//! channel returned by [`Bot::run`]. On every (re)connect the SDK back-fills
//! unread messages via the REST API, so nothing is lost while the bot was
//! offline. See `examples/echo.rs` for a minimal bot.

mod auth;
mod bot;
mod broadcast;
mod card;
mod collection;
mod command;
mod http;
mod message;
mod recovery;
mod session;
mod updates;

pub use bot::{Bot, Config};
pub use broadcast::Broadcast;
pub use card::CardMessage;
pub use collection::{CollectionField, CollectionItem, CollectionRequest, CollectionResult};
pub use command::{Command, CommandInput};
pub use http::Error;
pub use message::{
    EditOptions, Keyboard, KeyboardOption, LinkPreview, Message, MessageStream, PreviewImage,
    SendOptions,
};
pub use session::{Session, SessionSettings, SessionUser, SettingField};
pub use updates::Event;

#[cfg(test)]
mod tests;
