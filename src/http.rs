use std::time::Duration;

use reqwest::Method;
use serde::de::DeserializeOwned;

use crate::bot::Inner;

/// Errors returned by the SDK.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("authorization failed: {0}")]
    Auth(String),
    #[error("api error: {status} - {body}")]
    Api { status: u16, body: String },
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Inner {
    /// Performs an authenticated JSON request against the API, retrying once
    /// after `Retry-After` on 429.
    pub(super) async fn api(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response, Error> {
        let mut retried = false;
        loop {
            let token = self.token.read().await.clone();
            let mut req = self
                .http
                .request(method.clone(), format!("{}{path}", self.api_host))
                .header(reqwest::header::ACCEPT, "application/json")
                .bearer_auth(token);
            if let Some(body) = &body {
                req = req.json(body);
            }

            let resp = req.send().await?;
            if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && !retried {
                retried = true;
                let secs = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1)
                    .min(30);
                tokio::time::sleep(Duration::from_secs(secs)).await;
                continue;
            }
            return check(resp).await;
        }
    }

    pub(super) async fn api_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, Error> {
        Ok(self.api(method, path, body).await?.json().await?)
    }
}

/// Turns a non-2xx response into [`Error::Api`].
pub(super) async fn check(resp: reqwest::Response) -> Result<reqwest::Response, Error> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::Api {
            status: status.as_u16(),
            body: body.trim().to_string(),
        });
    }
    Ok(resp)
}
