use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;
use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::auth::{grant, regrant_loop, sub_from_token};
use crate::updates::listen;
use crate::{Command, Error, Event};

pub(super) const API_HOST: &str = "https://api.botcomm.app"; // resource API
const CHAN_BUFFER: usize = 64; // event channel buffer
const HTTP_TIMEOUT: Duration = Duration::from_secs(60); // timeout for REST API requests

/// Bot configuration for [`Bot::run`].
#[derive(Debug, Clone)]
pub struct Config {
    pub client_id: String,
    pub client_secret: String,
    pub startup_commands: Option<Vec<Command>>,
    pub input_mode: Option<String>,
    pub argument_commands: bool,
}

impl Config {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            startup_commands: None,
            input_mode: None,
            argument_commands: false,
        }
    }
}

/// A running BotComm bot: an authenticated client connected to the `/updates`
/// stream. Cheap to clone; all clones share the same connection.
#[derive(Clone)]
pub struct Bot {
    pub(super) inner: Arc<Inner>,
}

pub(super) struct Inner {
    pub(super) api_host: String,
    pub(super) id: String,
    pub(super) client_id: String,
    pub(super) client_secret: String,

    pub(super) http: reqwest::Client,
    pub(super) stream: reqwest::Client,
    pub(super) token: RwLock<String>,
    pub(super) commands: RwLock<HashMap<String, bool>>,
    pub(super) argument_commands: AtomicBool,
    // message IDs delivered by the latest backfill, pruned by dispatch
    pub(super) backfilled: Mutex<HashSet<String>>,
    pub(super) stop_tx: watch::Sender<bool>,
}

impl Bot {
    /// Authorizes the bot with its client credentials, connects to the
    /// `/updates` stream and starts listening. Returns an error if the
    /// initial grant or connect fails; after that the SDK keeps the token
    /// fresh and reconnects on its own. Incoming messages, commands and new
    /// sessions arrive on the returned channel.
    pub async fn run(config: Config) -> Result<(Bot, mpsc::Receiver<Event>), Error> {
        let http = reqwest::Client::builder().timeout(HTTP_TIMEOUT).build()?;
        let stream = reqwest::Client::builder()
            .connect_timeout(HTTP_TIMEOUT)
            .build()?;

        let tk = grant(&http, &config.client_id, &config.client_secret).await?;
        let id = sub_from_token(&tk.access_token)?;

        let (stop_tx, _) = watch::channel(false);
        let inner = Arc::new(Inner {
            api_host: API_HOST.to_string(),
            id,
            client_id: config.client_id,
            client_secret: config.client_secret,
            http,
            stream,
            token: RwLock::new(tk.access_token),
            argument_commands: AtomicBool::new(false),
            commands: RwLock::new(HashMap::new()),
            backfilled: Mutex::new(HashSet::new()),
            stop_tx,
        });

        let bot = Bot {
            inner: inner.clone(),
        };
        bot.set_argument_commands(config.argument_commands);
        if let Some(commands) = config.startup_commands {
            bot.register_commands(&commands).await?;
        }
        if let Some(mode) = config.input_mode {
            bot.set_input_mode(&mode).await?;
        }
        let resp = inner.connect().await?;

        let (tx, rx) = mpsc::channel(CHAN_BUFFER);
        tokio::spawn(regrant_loop(inner.clone(), tk.expires_in));
        tokio::spawn(listen(inner.clone(), tx, resp));

        Ok((Bot { inner }, rx))
    }

    /// The bot ID, resolved from the access token subject.
    pub fn id(&self) -> &str {
        &self.inner.id
    }

    /// Closes the `/updates` stream and stops the re-grant and listen tasks.
    pub fn stop(&self) {
        let _ = self.inner.stop_tx.send(true);
    }

    /// Controls whether apps show the bot's connection status.
    pub async fn set_online_status_visible(&self, visible: bool) -> Result<(), Error> {
        self.inner
            .api(
                Method::POST,
                &format!("/bot/{}", self.inner.id),
                Some(serde_json::json!({ "onlineStatusVisible": visible })),
            )
            .await?;
        Ok(())
    }
}
