use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::auth::{regrant_interval, sub_from_token, REGRANT_RETRY};
use crate::bot::{Inner, API_HOST};
use crate::recovery::backfill;
use crate::updates::{dispatch, Envelope};
use crate::*;

fn fake_jwt(payload: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
    format!("header.{encoded}.signature")
}

#[test]
fn sub_from_token_extracts_sub() {
    let token = fake_jwt(br#"{"sub":"bot123","scope":"bot"}"#);
    assert_eq!(sub_from_token(&token).unwrap(), "bot123");
}

#[test]
fn sub_from_token_rejects_bad_tokens() {
    assert!(sub_from_token("not-a-jwt").is_err());
    assert!(sub_from_token("a.b").is_err());
    let no_sub = fake_jwt(br#"{"scope":"bot"}"#);
    assert!(sub_from_token(&no_sub).is_err());
}

#[test]
fn regrant_interval_math() {
    assert_eq!(regrant_interval(3600), Duration::from_secs(3540));
    assert_eq!(regrant_interval(70), REGRANT_RETRY);
    assert_eq!(regrant_interval(0), REGRANT_RETRY);
    assert_eq!(regrant_interval(-5), REGRANT_RETRY);
}

#[test]
fn message_envelope_parses() {
    let frame = r#"{"type":"message","data":{"id":"m1","sessionID":"s1","authorID":"a1","content":"hi","silent":false,"read":false,"delivered":true,"authorTS":"2026-01-02T03:04:05Z","createTS":"2026-01-02T03:04:06.123456789Z"}}"#;
    let env: Envelope = serde_json::from_str(frame).unwrap();
    assert_eq!(env.kind, "message");
    let msg: Message = serde_json::from_value(env.data).unwrap();
    assert_eq!(msg.id, "m1");
    assert_eq!(msg.session_id, "s1");
    assert_eq!(msg.author_id, "a1");
    assert_eq!(msg.content, "hi");
    assert!(msg.delivered);
}

#[test]
fn collection_request_and_result_preserve_wire_contract() {
    let raw = r#"{"id":"r","sessionID":"s","key":"channels","action":"remove","itemID":"1","revision":4,"expiresTS":"2026-10-09T20:00:00Z"}"#;
    let request: CollectionRequest = serde_json::from_str(raw).unwrap();
    assert_eq!(request.session_id, "s");
    assert_eq!(request.item_id, "1");
    assert_eq!(request.revision, Some(4));
    assert_eq!(request.expires_ts.year(), 2026);
    let result = CollectionResult {
        revision: 5,
        changed: true,
        ..Default::default()
    };
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["items"], serde_json::json!([]));
    assert_eq!(value["revision"], 5);
    assert_eq!(value["changed"], true);
}

#[test]
fn keyboard_message_parses() {
    let raw = r#"{"id":"m1","sessionID":"s1","authorID":"a1","content":"Yes","keyboard":{"options":[{"label":"Yes","value":"yes"},{"label":"No","value":"no"}],"selected":"yes"},"replyTo":"m0","value":"yes","authorTS":"2026-01-02T03:04:05Z","createTS":"2026-01-02T03:04:06Z"}"#;
    let msg: Message = serde_json::from_str(raw).unwrap();
    let keyboard = msg.keyboard.unwrap();
    assert_eq!(keyboard.options.len(), 2);
    assert_eq!(keyboard.options[0].label, "Yes");
    assert_eq!(keyboard.options[0].value, "yes");
    assert_eq!(keyboard.selected.as_deref(), Some("yes"));
    assert_eq!(msg.reply_to.as_deref(), Some("m0"));
    assert_eq!(msg.value.as_deref(), Some("yes"));
}

#[test]
fn session_envelope_parses() {
    let frame = r#"{"type":"new_session","data":{"id":"s1","unreadCount":0,"createTS":"2026-01-02T03:04:05Z"}}"#;
    let env: Envelope = serde_json::from_str(frame).unwrap();
    assert_eq!(env.kind, "new_session");
    let session: Session = serde_json::from_value(env.data).unwrap();
    assert_eq!(session.id, "s1");
    assert!(session.last_message.is_none());
}

#[test]
fn zero_go_timestamp_parses() {
    // Go marshals the zero time.Time as 0001-01-01T00:00:00Z
    let raw = r#"{"id":"m1","sessionID":"s1","authorID":"a1","content":"hi","authorTS":"0001-01-01T00:00:00Z","createTS":"2026-01-02T03:04:05Z"}"#;
    let msg: Message = serde_json::from_str(raw).unwrap();
    assert_eq!(msg.author_ts.year(), 1);
}

#[cfg(test)]
mod recovery_contract {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    #[tokio::test]
    async fn all_pages_order_ack_and_dedup() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/recovery.json")).unwrap();
        let messages = fixture["messages"].as_array().unwrap().clone();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, mut rx) = mpsc::channel(512);
        let server_tx = tx.clone();
        let server = std::thread::spawn(move || {
            let mut reads = Vec::new();
            // One sessions request, three pages and 205 acknowledgements.
            for _ in 0..209 {
                let (mut socket, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let parts: Vec<_> = line.split_whitespace().collect();
                let method = parts[0];
                let url = reqwest::Url::parse(&format!("http://test{}", parts[1])).unwrap();
                let body = if url.path() == "/session" {
                    serde_json::json!([{
                        "id": "s",
                        "unreadCount": 205,
                        "createTS": "2026-09-01T00:00:00Z",
                    }])
                } else if method == "POST" {
                    assert!(
                        512 - server_tx.capacity() > reads.len(),
                        "ack before enqueue"
                    );
                    reads.push(url.path().split('/').nth(4).unwrap().to_string());
                    serde_json::json!({})
                } else {
                    let query: std::collections::HashMap<_, _> =
                        url.query_pairs().into_owned().collect();
                    assert_eq!(query.get("order").unwrap(), "asc");

                    let start = query
                        .get("afterID")
                        .map(|id| {
                            messages
                                .iter()
                                .position(|m| m["id"].as_str() == Some(id))
                                .unwrap()
                                + 1
                        })
                        .unwrap_or(0);
                    if start > 0 {
                        assert!(query.contains_key("afterTS"));
                    }

                    serde_json::json!(&messages[start..(start + 100).min(messages.len())])
                }
                .to_string();

                // Consume headers before closing the connection.
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h == "\r\n" || h.is_empty() {
                        break;
                    }
                }
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body,
                )
                .unwrap();
            }
            reads
        });
        let (stop_tx, _) = watch::channel(false);
        let inner = Arc::new(Inner {
            api_host: format!("http://{addr}"),
            id: "bot".into(),
            client_id: "id".into(),
            client_secret: "secret".into(),
            http: reqwest::Client::new(),
            stream: reqwest::Client::new(),
            token: RwLock::new("token".into()),
            argument_commands: AtomicBool::new(false),
            commands: RwLock::new(HashMap::from([("/help".to_string(), false)])),
            backfilled: Mutex::new(HashSet::new()),
            stop_tx,
        });
        backfill(&inner, &tx).await;
        let reads = server.join().unwrap();
        for (i, id) in reads.iter().enumerate() {
            assert_eq!(id, &format!("m{i:03}"));
        }
        assert_eq!(reads.len(), 205);
        let duplicate =
            serde_json::json!({"type": "message", "data": fixture["messages"][204]}).to_string();
        assert!(dispatch(&inner, &duplicate, &tx).await.is_none());
        let mut total = 0;
        while rx.try_recv().is_ok() {
            total += 1;
        }
        assert_eq!(total, 205);
    }
}

#[cfg(test)]
mod input_mode_tests {
    use super::*;

    #[tokio::test]
    async fn argument_routing_is_opt_in_and_keeps_keyboard_values_exact() {
        for enabled in [false, true] {
            let (stop_tx, _) = watch::channel(false);
            let inner = Inner {
                api_host: API_HOST.into(),
                id: "bot".into(),
                client_id: "id".into(),
                client_secret: "secret".into(),
                http: reqwest::Client::new(),
                stream: reqwest::Client::new(),
                token: RwLock::new(String::new()),
                commands: RwLock::new(HashMap::from([
                    ("/now".into(), false),
                    ("/city".into(), true),
                ])),
                argument_commands: AtomicBool::new(enabled),
                backfilled: Mutex::new(HashSet::new()),
                stop_tx,
            };
            let (tx, mut rx) = mpsc::channel(8);
            for (content, value, reply_to, command, argument, exact) in [
                ("/now", "", "", Some("/now"), Some(""), true),
                ("/city", "", "", Some("/city"), Some(""), true),
                (
                    "/city \tNew  York 🙂\r\n",
                    "",
                    "",
                    Some("/city"),
                    Some("New  York 🙂"),
                    false,
                ),
                ("/citywrong Warsaw", "", "", None, None, false),
                ("/now args", "", "", None, None, false),
                ("Label", "/now", "keyboard", None, None, true),
                ("Label", "/city Warsaw", "keyboard", None, None, false),
                ("/city Warsaw", "", "keyboard", None, None, false),
            ] {
                let msg: Message = serde_json::from_value(serde_json::json!({
                    "id": "m", "sessionID": "s", "authorID": "u", "content": content,
                    "value": value, "replyTo": reply_to,
                    "authorTS": "2026-01-01T00:00:00Z", "createTS": "2026-01-01T00:00:00Z"
                }))
                .unwrap();
                assert!(inner.route(msg, &tx).await);
                let event = rx.recv().await.unwrap();
                let is_command = matches!(&event, Event::Command(_));
                assert_eq!(is_command, exact || (enabled && command.is_some()));
                let msg = match event {
                    Event::Command(msg) | Event::Message(msg) => msg,
                    _ => panic!("unexpected event"),
                };
                assert_eq!(msg.content, content);
                assert_eq!(msg.command.as_deref(), if enabled { command } else { None });
                assert_eq!(
                    msg.argument.as_deref(),
                    if enabled { argument } else { None }
                );
            }
        }
    }
}
