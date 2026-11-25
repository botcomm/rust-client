//! A minimal BotComm bot that echoes every message back to the sender.
//!
//! Configuration (environment):
//!
//! - `CLIENT_ID`     - bot client id (shown once on bot creation)
//! - `CLIENT_SECRET` - bot client secret (shown once on bot creation)

use botcomm::{Bot, Command, Config, Event, SettingField};

#[tokio::main]
async fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    let client_id = std::env::var("CLIENT_ID").expect("CLIENT_ID is required");
    let client_secret = std::env::var("CLIENT_SECRET").expect("CLIENT_SECRET is required");

    let mut config = Config::new(client_id, client_secret);
    config.input_mode = Some("messages".into());
    let (bot, mut events) = Bot::run(config)
        .await
        .expect("failed to start bot");
    log::info!("echo bot {} is running", bot.id());

    bot.register_commands(&[Command {
        input: None,
        name: "Ping".into(),
        value: "/ping".into(),
        description: "Check the bot is alive".into(),
    }])
    .await
    .expect("failed to register commands");
    bot.register_settings(&[SettingField {
        key: "shout".into(),
        label: "Shout back".into(),
        description: Some("Echo in upper case".into()),
        kind: "toggle".into(),
        options: vec![],
        default: Some("false".into()),
    }])
    .await
    .expect("failed to register settings");

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Some(Event::Message(msg)) => {
                    let _ = bot.typing(&msg.session_id).await;
                    match bot.send(&msg.session_id, &msg.content, false).await {
                        Ok(sent) => log::info!("echoed as {}", sent.id),
                        Err(err) => log::error!("failed to reply: {err}"),
                    }
                }
                Some(Event::Command(msg)) if msg.content == "/ping" => {
                    if let Err(err) = bot.send(&msg.session_id, "pong", false).await {
                        log::error!("failed to reply: {err}");
                    }
                }
                Some(Event::Command(msg)) => {
                    let reply = format!("command received: {}", msg.content);
                    if let Err(err) = bot.send(&msg.session_id, &reply, false).await {
                        log::error!("failed to reply: {err}");
                    }
                }
                Some(Event::CollectionRequest(_)) => {}
                Some(Event::Reaction(_)) => {}
                Some(Event::Settings(settings)) => {
                    log::info!("settings of {} changed: {:?}", settings.session_id, settings.settings);
                }
                Some(Event::NewSession(session)) => {
                    log::info!("new session {}", session.id);
                    if let Err(err) = bot
                        .send(&session.id, "Hello! I repeat everything you say.", false)
                        .await
                    {
                        log::error!("failed to greet: {err}");
                    }
                }
                None => break,
            },
            _ = tokio::signal::ctrl_c() => {
                bot.stop();
                break;
            }
        }
    }
}
