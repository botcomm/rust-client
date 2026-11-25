use botcomm::Message;

#[test]
fn messages_decode_with_and_without_previews() {
    let mut body = serde_json::json!({
        "id": "m1",
        "sessionID": "s1",
        "authorID": "bot",
        "content": "https://example.com",
        "authorTS": "2026-10-09T00:00:00Z",
        "createTS": "2026-10-09T00:00:00Z",
    });
    let legacy: Message = serde_json::from_value(body.clone()).unwrap();
    assert!(legacy.link_preview.is_none());
    body["linkPreview"] = serde_json::json!({
        "url": "https://example.com",
        "siteName": "Example",
        "title": "Article",
        "image": {"id": "image", "width": 800, "height": 400},
    });
    body["previewTS"] = "2026-10-09T00:00:05Z".into();
    let message: Message = serde_json::from_value(body).unwrap();
    let preview = message.link_preview.unwrap();
    assert_eq!(preview.site_name, "Example");
    assert_eq!(preview.image.unwrap().width, 800);
    assert!(message.preview_ts.is_some());
}
