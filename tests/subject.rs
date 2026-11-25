use botcomm::{EditOptions, Message, SendOptions};

#[test]
fn subject_options_preserve_omitted_fields_and_allow_clearing() {
    let options = SendOptions {
        subject: Some("Weather".into()),
        ..Default::default()
    };
    let body = serde_json::to_value(options).unwrap();
    assert_eq!(body["subject"], "Weather");
    assert!(body.get("keyboard").is_none());
    let edit = EditOptions {
        subject: Some(String::new()),
        ..Default::default()
    };
    assert_eq!(serde_json::to_value(edit).unwrap(), serde_json::json!({"subject": ""}));
}

#[test]
fn messages_decode_with_and_without_subjects() {
    let mut body = serde_json::json!({
        "id": "m1", "sessionID": "s1", "authorID": "bot", "content": "Sunny",
        "authorTS": "2026-10-03T00:00:00Z", "createTS": "2026-10-03T00:00:00Z",
    });
    let legacy: Message = serde_json::from_value(body.clone()).unwrap();
    assert!(legacy.subject.is_none());
    body["subject"] = "Weather".into();
    let message: Message = serde_json::from_value(body).unwrap();
    assert_eq!(message.subject.as_deref(), Some("Weather"));
}

#[test]
fn card_messages_are_opt_in_and_legacy_decoding_still_works() {
    let body = serde_json::json!({
        "id": "m1", "sessionID": "s1", "authorID": "bot", "content": "Plan: running",
        "authorTS": "2026-11-04T00:00:00Z", "createTS": "2026-11-04T00:00:00Z",
        "card": {"kind": "plan", "title": "Deploy", "status": "running", "revision": 2},
        "fields": {"targets": ["one"]},
    });
    let legacy: Message = serde_json::from_value(body.clone()).unwrap();
    assert_eq!(legacy.content, "Plan: running");
    assert_eq!(legacy.fields["targets"][0], "one");
    assert_eq!(legacy.card.as_ref().unwrap()["revision"], 2);
    let extended: botcomm::CardMessage = serde_json::from_value(body).unwrap();
    assert_eq!(extended.card.unwrap()["revision"], 2);
    assert_eq!(extended.fields["targets"][0], "one");
}

#[test]
fn metadata_options_distinguish_omitted_object_and_null() {
    let metadata = serde_json::json!({"debug": [true, null, {"ms": 12}]});
    let send = SendOptions {
        metadata: Some(metadata.clone()),
        ..Default::default()
    };
    assert_eq!(serde_json::to_value(send).unwrap()["metadata"], metadata);
    assert!(serde_json::to_value(EditOptions::default()).unwrap().get("metadata").is_none());
    for value in [metadata, serde_json::json!({}), serde_json::Value::Null] {
        let edit = EditOptions {
            metadata: Some(value.clone()),
            ..Default::default()
        };
        assert_eq!(serde_json::to_value(edit).unwrap(), serde_json::json!({"metadata": value}));
    }
}
