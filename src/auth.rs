use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use serde::Deserialize;

use crate::bot::Inner;
use crate::http::check;
use crate::Error;

const AUTH_HOST: &str = "https://oauth.botcomm.app"; // client_credentials token endpoint
pub(super) const REGRANT_RETRY: Duration = Duration::from_secs(10); // retry interval after a failed re-grant
const REGRANT_EARLINESS: i64 = 60; // re-grant this many seconds before expiry

#[derive(Debug, Deserialize)]
pub(super) struct Token {
    pub(super) access_token: String,
    #[serde(default)]
    pub(super) expires_in: i64,
}

/// Exchanges the client credentials for an access token. Bots get no refresh
/// token: they re-grant before expiry.
pub(super) async fn grant(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
) -> Result<Token, Error> {
    let resp = http
        .post(format!("{AUTH_HOST}/token"))
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await?;

    let tk: Token = check(resp).await?.json().await?;
    if tk.access_token.is_empty() {
        return Err(Error::Auth("empty access token in response".into()));
    }
    Ok(tk)
}

/// Extracts the `sub` claim from a JWT without verifying it (the server is
/// the source of the token).
pub(super) fn sub_from_token(token: &str) -> Result<String, Error> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(Error::Auth("invalid token format".into()));
    }

    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|err| Error::Auth(format!("failed to decode token payload: {err}")))?;
    let claims: serde_json::Value = serde_json::from_slice(&payload)?;

    claims["sub"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Auth("sub claim not found in token".into()))
}

pub(super) fn regrant_interval(expires_in: i64) -> Duration {
    let secs = expires_in.saturating_sub(REGRANT_EARLINESS).max(0) as u64;
    Duration::from_secs(secs).max(REGRANT_RETRY)
}

/// Re-grants [`REGRANT_EARLINESS`] before every expiry, retrying every
/// [`REGRANT_RETRY`] on failure.
pub(super) async fn regrant_loop(inner: Arc<Inner>, initial_expires_in: i64) {
    let mut stop = inner.stop_tx.subscribe();
    let mut interval = regrant_interval(initial_expires_in);

    loop {
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(interval) => {}
        }

        match grant(&inner.http, &inner.client_id, &inner.client_secret).await {
            Ok(tk) => {
                *inner.token.write().await = tk.access_token;
                interval = regrant_interval(tk.expires_in);
            }
            Err(err) => {
                log::error!("failed to refresh access token: {err}");
                interval = REGRANT_RETRY;
            }
        }
    }
}
