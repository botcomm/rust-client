use std::collections::HashSet;
use std::sync::Arc;

use reqwest::Method;
use tokio::sync::mpsc;

use crate::bot::{Inner, API_HOST};
use crate::{Error, Event, Message};

impl Inner {
    pub(super) async fn load_unread(
        &self,
        session_id: &str,
        after: Option<&Message>,
    ) -> Result<Vec<Message>, Error> {
        let mut url =
            reqwest::Url::parse(&format!("{API_HOST}/session/{session_id}/message/unread"))
                .expect("valid API URL");
        url.query_pairs_mut().append_pair("order", "asc");
        if let Some(msg) = after {
            let ts = msg
                .create_ts
                .format(&time::format_description::well_known::Rfc3339)
                .expect("valid timestamp");
            url.query_pairs_mut()
                .append_pair("afterID", &msg.id)
                .append_pair("afterTS", &ts);
        }
        let path = format!("{}?{}", url.path(), url.query().unwrap_or_default());
        self.api_json(Method::GET, &path, None).await
    }

    pub(super) async fn confirm_read(&self, session_id: &str, message_id: &str) {
        let path = format!("/session/{session_id}/message/{message_id}/read");
        if let Err(err) = self.api(Method::POST, &path, None).await {
            log::error!("failed to confirm message read: {err}");
        }
    }
}

/// Loads the sessions and re-delivers messages that arrived while the bot was
/// offline, oldest first, confirming each as read.
pub(super) async fn backfill(inner: &Arc<Inner>, tx: &mpsc::Sender<Event>) {
    let sessions = match inner.sessions().await {
        Ok(sessions) => sessions,
        Err(err) => {
            log::error!("failed to get sessions: {err}");
            return;
        }
    };

    let mut delivered = HashSet::new();
    for session in sessions {
        if session.unread_count == 0 {
            continue;
        }
        let mut after: Option<Message> = None;
        loop {
            let unread = match inner.load_unread(&session.id, after.as_ref()).await {
                Ok(unread) => unread,
                Err(err) => {
                    log::error!("failed to load unread messages: {err}");
                    break;
                }
            };
            let count = unread.len();
            let last = unread.last().cloned();
            for msg in unread {
                if !delivered.insert(msg.id.clone()) {
                    continue;
                }
                let (session_id, message_id) = (msg.session_id.clone(), msg.id.clone());
                if !inner.route(msg, tx).await {
                    return;
                }
                inner.confirm_read(&session_id, &message_id).await;
            }
            if count < 100 || last.as_ref().map(|m| &m.id) == after.as_ref().map(|m| &m.id) {
                break;
            }
            after = last;
        }
    }

    // remember what was just delivered so dispatch can drop a duplicate that
    // arrived over the stream while the backfill was running
    *inner.backfilled.lock().await = delivered;
}
