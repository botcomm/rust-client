use reqwest::Method;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{Bot, Error};

/// A bot-managed list rendered in native chat settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionField {
    pub key: String,
    pub label: String,
    pub max_items: usize,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub placeholder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionItem {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
}

/// Compare/increment revision atomically on mutation; reject expired requests.
#[derive(Debug, Clone, Deserialize)]
pub struct CollectionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub key: String,
    pub action: String,
    #[serde(default)]
    pub value: String,
    #[serde(default, rename = "itemID")]
    pub item_id: String,
    pub revision: Option<i64>,
    #[serde(rename = "expiresTS", with = "time::serde::rfc3339")]
    pub expires_ts: OffsetDateTime,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionResult {
    pub items: Vec<CollectionItem>,
    pub revision: i64,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub changed: bool,
}

impl Bot {
    pub async fn register_collections(&self, fields: &[CollectionField]) -> Result<(), Error> {
        self.inner
            .api(
                Method::POST,
                &format!("/bot/{}", self.inner.id),
                Some(serde_json::json!({ "collections": fields })),
            )
            .await?;
        Ok(())
    }

    pub async fn respond_collection(
        &self,
        request: &CollectionRequest,
        result: &CollectionResult,
    ) -> Result<(), Error> {
        self.inner
            .api(
                Method::POST,
                &format!(
                    "/session/{}/collections/{}/result/{}",
                    request.session_id, request.key, request.id
                ),
                Some(serde_json::json!(result)),
            )
            .await?;
        Ok(())
    }
}
