//! The model additions over a scripted local HTTP server: the refresh a
//! published additions file goes through, which no run of the binary can
//! reach before that file exists at its URL.
//!
//! What can go wrong there: a file emptied because the catalog caught up
//! is taken for a failed fetch and the stale additions stay forever; a
//! missing file or one from a future schema wipes the additions already
//! served; the next process forgets what this one fetched.

mod common;

use serde_json::{json, Value};
use uh_models::ModelCatalog;

fn payload(ids: &[&str]) -> String {
    let compiled = uh_models::transports::compiled_models();
    let anthropic = compiled.iter().find(|m| m.provider == "anthropic").unwrap();
    let models: Vec<Value> = ids
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "name": id,
                "api": anthropic.api,
                "provider": anthropic.provider,
                "baseUrl": anthropic.base_url,
                "reasoning": false,
                "input": ["text"],
                "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
                "contextWindow": 128_000,
                "maxTokens": 4_096,
            })
        })
        .collect();
    json!({"schemaVersion": 1, "models": models}).to_string()
}

fn has(catalog: &ModelCatalog, id: &str) -> bool {
    catalog.resolve(None).iter().any(|model| model.id == id)
}

#[tokio::test]
async fn a_refresh_follows_the_published_file_and_failures_keep_the_last_one() {
    let dir = tempfile::tempdir().unwrap();
    let catalog_server =
        common::MockServer::start(vec![common::ok_json(payload(&["listed"]), None)]).await;
    let future_schema = json!({"schemaVersion": 2, "models": []}).to_string();
    let additions_server = common::MockServer::start(vec![
        common::ok_json(payload(&["added"]), None),
        common::status(404, "Not Found"),
        common::ok_json(future_schema, None),
        common::ok_json(payload(&[]), None),
    ])
    .await;
    let open = || {
        ModelCatalog::with_urls(
            Some(dir.path().join("models")),
            Some(dir.path().to_path_buf()),
            &catalog_server.url("/catalog"),
            &catalog_server.url("/api/v1"),
        )
        .with_additions_url(&additions_server.url("/additions"))
    };
    let catalog = open();
    assert!(!has(&catalog, "added"), "nothing fetched yet");

    catalog.refresh(true).await;
    assert!(has(&catalog, "listed") && has(&catalog, "added"));
    assert!(has(&open(), "added"), "the next process reads the cache");

    catalog.refresh(true).await;
    assert!(has(&catalog, "added"), "a 404 keeps the last additions");
    catalog.refresh(true).await;
    assert!(has(&catalog, "added"), "a future schema keeps them too");

    catalog.refresh(true).await;
    assert!(!has(&catalog, "added"), "an emptied file removes them");
    assert!(has(&catalog, "listed"));
    assert!(!has(&open(), "added"), "and the next process agrees");
    assert_eq!(additions_server.request_count(), 4);
}
