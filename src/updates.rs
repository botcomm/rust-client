use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::bot::{Inner, API_HOST};
use crate::http::check;
use crate::recovery::backfill;
use crate::{CollectionRequest, Error, Message, Session, SessionSettings};

const RECONNECT_DELAY: Duration = Duration::from_secs(5); // pause between reconnect attempts
const READ_WAIT: Duration = Duration::from_secs(90); // read deadline, reset by server keepalives

/// An incoming event delivered on the channel returned by [`crate::Bot::run`].
#[derive(Debug, Clone)]
pub enum Event {
    /// An incoming message that is not a registered command.
    Message(Message),
    /// The current reactions on a message changed.
    Reaction(Message),
    /// An incoming message matching a registered command value.
    Command(Message),
    /// A user opened a new session with the bot.
    NewSession(Session),
    /// The settings of a session changed (by the user or the bot).
    Settings(SessionSettings),
    /// Native settings list/add/remove request; reply with respond_collection.
    CollectionRequest(CollectionRequest),
}

#[derive(Debug, Deserialize)]
pub(super) struct Envelope {
    #[serde(rename = "type")]
    pub(super) kind: String,
    #[serde(default)]
    pub(super) data: serde_json::Value,
}

impl Inner {
    /// Opens the `/updates` event stream with the current access token.
    pub(super) async fn connect(&self) -> Result<reqwest::Response, Error> {
        let token = self.token.read().await.clone();
        let resp = self
            .stream
            .get(format!("{API_HOST}/updates"))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .bearer_auth(token)
            .send()
            .await?;
        check(resp).await
    }
}

/// Back-fills unread messages, reads events until the stream breaks and
/// reconnects every [`RECONNECT_DELAY`] until the bot is stopped.
pub(super) async fn listen(
    inner: Arc<Inner>,
    tx: mpsc::Sender<Event>,
    mut resp: reqwest::Response,
) {
    let mut stop = inner.stop_tx.subscribe();

    loop {
        tokio::select! {
            _ = stop.changed() => return,
            _ = backfill(&inner, &tx) => {}
        }

        let mut stream = resp.bytes_stream();
        let mut buf = Vec::new();
        let mut data = String::new();
        loop {
            let chunk = tokio::select! {
                _ = stop.changed() => return,
                chunk = tokio::time::timeout(READ_WAIT, stream.next()) => chunk,
            };
            match chunk {
                Ok(Some(Ok(bytes))) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                        let line = String::from_utf8_lossy(&buf[..pos])
                            .trim_end_matches('\r')
                            .to_string();
                        buf.drain(..=pos);
                        if let Some(payload) = line.strip_prefix("data:") {
                            data.push_str(payload.strip_prefix(' ').unwrap_or(payload));
                        } else if line.is_empty() && !data.is_empty() {
                            if let Some((session_id, message_id)) =
                                dispatch(&inner, &data, &tx).await
                            {
                                let inner = inner.clone();
                                tokio::spawn(async move {
                                    inner.confirm_read(&session_id, &message_id).await
                                });
                            }
                            data.clear();
                        }
                    }
                }
                Ok(Some(Err(err))) => {
                    log::warn!("updates stream read failed: {err}");
                    break;
                }
                Ok(None) => {
                    log::warn!("updates stream closed by the server");
                    break;
                }
                Err(_) => {
                    log::warn!("updates stream timed out");
                    break;
                }
            }
        }

        if *stop.borrow() {
            return;
        }

        loop {
            match inner.connect().await {
                Ok(r) => {
                    resp = r;
                    log::info!("reconnected to the updates stream");
                    break;
                }
                Err(err) => {
                    log::warn!("failed to reconnect: {err}");
                    tokio::select! {
                        _ = stop.changed() => return,
                        _ = tokio::time::sleep(RECONNECT_DELAY) => {}
                    }
                }
            }
        }
    }
}

/// Parses one event and routes it: `message` envelopes go to the event
/// channel (returning the session/message IDs for read confirmation),
/// `new_session` becomes [`Event::NewSession`], `settings` [`Event::Settings`].
pub(super) async fn dispatch(
    inner: &Arc<Inner>,
    frame: &str,
    tx: &mpsc::Sender<Event>,
) -> Option<(String, String)> {
    let env: Envelope = match serde_json::from_str(frame) {
        Ok(env) => env,
        Err(err) => {
            log::warn!("failed to parse envelope: {err}");
            return None;
        }
    };

    match env.kind.as_str() {
        "message" => {
            let msg: Message = match serde_json::from_value(env.data) {
                Ok(msg) => msg,
                Err(err) => {
                    log::warn!("failed to parse message: {err}");
                    return None;
                }
            };
            if inner.backfilled.lock().await.remove(&msg.id) {
                return None; // already delivered (and confirmed read) by the backfill
            }
            let ids = (msg.session_id.clone(), msg.id.clone());
            if !inner.route(msg, tx).await {
                return None;
            }
            Some(ids)
        }
        "reaction" => {
            if let Ok(message) = serde_json::from_value::<Message>(env.data) {
                let _ = tx.try_send(Event::Reaction(message));
            }
            None
        }
        "new_session" => {
            match serde_json::from_value::<Session>(env.data) {
                Ok(session) => {
                    if tx.send(Event::NewSession(session)).await.is_err() {
                        log::warn!("event receiver dropped, discarding event");
                    }
                }
                Err(err) => log::warn!("failed to parse session: {err}"),
            }
            None
        }
        "collection_request" => {
            match serde_json::from_value::<CollectionRequest>(env.data) {
                Ok(request) => {
                    if tx.send(Event::CollectionRequest(request)).await.is_err() {
                        log::warn!("event receiver dropped, discarding event");
                    }
                }
                Err(err) => log::warn!("failed to parse collection request: {err}"),
            }
            None
        }
        "settings" => {
            match serde_json::from_value::<SessionSettings>(env.data) {
                Ok(settings) => {
                    if tx.send(Event::Settings(settings)).await.is_err() {
                        log::warn!("event receiver dropped, discarding event");
                    }
                }
                Err(err) => log::warn!("failed to parse settings: {err}"),
            }
            None
        }
        "read" => None, // the counterpart read one of the bot's messages: nothing to do
        other => {
            log::warn!("unknown envelope type {other:?}");
            None
        }
    }
}
