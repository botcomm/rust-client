# BotComm Rust SDK

Rust SDK for building [BotComm](https://botcomm.app) bots: client-credentials auth with automatic re-grant, a resilient `/updates` Server-Sent Events stream, unread back-fill on every (re)connect and command routing.

## Quickstart

Create a bot in the [BotComm console](https://web.botcomm.app/console/bots) and save its credentials. In the same terminal where you'll run the bot, set:

```sh
export CLIENT_ID='your-client-id'
export CLIENT_SECRET='your-client-secret'
```

Create a binary project and add the SDK and async runtime:

```sh
cargo new echo-bot
cd echo-bot
cargo add botcomm
cargo add tokio --features macros,rt-multi-thread
```

Replace `src/main.rs` with:

```rust
use botcomm::{Bot, Config, Event};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new(env::var("CLIENT_ID")?, env::var("CLIENT_SECRET")?);
    config.input_mode = Some("messages".into());

    let (bot, mut events) = Bot::run(config).await?;
    while let Some(event) = events.recv().await {
        if let Event::Message(msg) = event {
            bot.send(&msg.session_id, &msg.content, false).await?;
        }
    }
    Ok(())
}
```

Run `cargo run`, open your bot’s chat in BotComm, and send `hello`. The bot should reply with `hello`.

The [full echo example](https://github.com/botcomm/rust-client/blob/HEAD/examples/echo.rs) also handles commands and settings.

## Source layout

`src/lib.rs` preserves the public crate exports. `bot.rs` contains configuration and
lifecycle; `message.rs`, `card.rs`, `broadcast.rs`, `command.rs`, `session.rs` and
`collection.rs` keep feature types and methods together. `auth.rs` and `http.rs`
handle authentication and requests; `updates.rs` and `recovery.rs` handle streaming
and unread recovery. Existing unit tests live in `tests.rs`, with public API tests
in `tests/`.

## API

- `Config::new(client_id, client_secret)` — configuration.
- `Bot::run(config) -> (Bot, mpsc::Receiver<Event>)` — grants, connects and starts listening; fails fast if the initial grant or connect fails, self-heals afterwards (re-grant one minute before token expiry, reconnect every 5s).
- `Event` — `Message` (regular message), `Command` (content matches a registered command value), `NewSession` (a user opened a chat with the bot), `Settings` (a session's settings changed).
- `bot.send(session_id, content, silent) -> Message` — post a reply into a session; the returned message carries the `id` needed to edit or delete it.
- `bot.send_keyboard(session_id, content, options) -> Message` — post a message with one-tap answer options, see [Keyboards](#keyboards).
- `bot.edit(session_id, message_id, content, keyboard) -> Message` — change a bot-authored message (`content`, `keyboard` or both); replacing an answered keyboard fails with 409.
- `bot.delete(session_id, message_id)` — remove a bot-authored message.
- `bot.typing(session_id)` — show a typing indicator for a few seconds; repeat every 3-4s while working.
- `bot.broadcast(content, keyboard, silent) -> Broadcast` / `bot.broadcast_status(id)` — send one message to each connected account’s oldest chat as a paced server-side job; each user gets their own copy and keyboard.
- `bot.register_settings(&[SettingField])` — publish the per-session settings schema (`toggle`, `select`, `text`, `time`); every `Session` carries the effective `settings`, changes arrive as `Event::Settings`.
- `bot.set_settings(session_id, values) -> SessionSettings` — change a session's settings on the user's behalf.
- `bot.register_commands(&[Command])` — publish commands and enable command routing.
- `bot.sessions()` — list the bot's sessions (`user.timezone`/`user.locale` when the client reported them, `muted` when the user silenced push).
- `bot.stop()` — close the connection and stop the background tasks.

## Subjects

```rust
use botcomm::{EditOptions, SendOptions};

bot.send_with_options(session_id, "Your deployment finished!", SendOptions {
    subject: Some("Deployment".into()),
    ..Default::default()
}).await?;
bot.edit_with_options(session_id, message_id, EditOptions {
    subject: Some(String::new()), // clear the subject
    ..Default::default()
}).await?;
```

Subjects are optional single-line plain text, up to 256 Unicode characters. `None` preserves the current subject during edits. `SendOptions` also supports a keyboard and silent delivery, and can be passed to `broadcast_with_options`.

## Keyboards

Offer one-tap answers by attaching a keyboard to a message:

```rust,no_run
use botcomm::KeyboardOption;

bot.send_keyboard(&msg.session_id, "Deploy to production?", &[
    KeyboardOption { label: "Yes".into(), value: "deploy".into() },
    KeyboardOption { label: "No".into(), value: "cancel".into() },
]).await?;
```

The tap arrives as an `Event::Message` whose `content` is the chosen option's `label`, with `reply_to` set to the keyboard message's id and `value` to the option's machine value; a `value` matching a registered command arrives as `Event::Command` instead. The server stamps `keyboard.selected` on the original message once answered.

Writes are rate limited per session and per bot; a `429` is retried once after the server's `Retry-After`, then surfaces as `Error::Api`.

Incoming messages are confirmed as read automatically. On every (re)connect the SDK back-fills unread messages via the REST API (oldest first) and dedups anything that also arrives over the socket, so nothing is lost while the bot was offline.


## Commands-only input

```rust
use botcomm::{Bot, Command, CommandInput, Config};

let mut config = Config::new(client_id, client_secret);
config.argument_commands = true;
config.input_mode = Some("commands".into());
config.startup_commands = Some(vec![Command {
    name: "Set city".into(), value: "/city".into(), description: "Change city".into(),
    input: Some(CommandInput {
        label: "City".into(), placeholder: "Warsaw".into(), required: true,
        ..Default::default()
    }),
}]);
let (bot, mut events) = Bot::run(config).await?;
// Event::Command(msg): msg.command / msg.argument are Some values;
// msg.content remains the complete original message.
```

`bot.set_input_mode("messages").await?` enables ordinary messages.
`bot.set_argument_commands(true)` also enables argument routing at runtime.
Use `input: None` in command literals without forms; use `Config::new` for default
startup options.


## Online status visibility

After `Bot::run`, use `bot.set_online_status_visible(true).await?` to show the bot's online/offline indicator in the web and iOS apps, or `false` to hide it. The setting defaults to hidden and persists between restarts.

## Receive-only mode and collections

Use input mode `none` for a receive-only chat. Session settings remain available; user messages, commands, and keyboard answers are rejected. Publish native session collections with `register_collections`, `Event::CollectionRequest`, and `respond_collection`. Register the schema before enabling `none`. Persist and atomically compare/increment list revisions on mutations, reject expired requests, and return results outside chat history.

## Streaming replies and reactions

`start_stream(session_id)` creates an empty live bubble. Call `update_stream(session_id, message_id, accumulated_text, sequence, "active")` about every 500ms, then finish with `"completed"` or `"failed"`. Sequence numbers increase and identical snapshot retries are safe. Idle streams expire after two minutes; send unchanged heartbeats during provider pauses. `SendOptions` also supports `stream: true`.

`react(session_id, message_id, "❤️")` replaces the bot's reaction; `""` removes it. `Event::Reaction(message)` carries live reaction snapshots, separate from incoming messages. Fetch messages after reconnect to reconcile reactions missed while offline.


## Agent cards

Use `send_message` and `update_message` with a `serde_json::Value` payload
containing `{"card": ...}`. Card fields use the camelCase JSON names below.

| Kind | Initial status | Other statuses |
|---|---|---|
| `plan` | `queued` | `running`, `waiting`, `completed`, `failed`, `cancelled` |
| `approval` | `pending` | `approved`, `rejected`, `expired`, `cancelled` |
| `form` | `pending` | `submitted`, `expired`, `cancelled` |
| `artifact` | `completed` | None; immutable |

The server sets the initial status and revision. Plan updates require
the latest `revision` and a status other than `queued`; completed, failed and
cancelled plans cannot be reopened. Stale revisions return 409. Approval/form
updates only cancel pending, unexpired requests; answers and expiration determine
their other statuses. Artifact cards cannot be updated.

An artifact requires an HTTP(S) `url` without credentials and may include
`parentID`, the message ID of an existing plan from the same bot in the same chat.

`sourceID` is different: only plans accept it, pointing to an approved approval
or submitted form from the same bot and chat. Each source permits one plan.
The server sets `planID` on that source; never supply `planID` yourself.
Links cannot be edited, and linking cards does not execute work.

A plan can contain up to 50 tasks. Updating its task list replaces the full list.
