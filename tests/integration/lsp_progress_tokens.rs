//! Work-done progress of concurrent requests.
//!
//! When the client sends no `workDoneToken`, the server creates one per
//! request for its "Go to Implementation" and "Find References" progress.
//! Editors send such requests concurrently (an implementation gutter asks
//! for every declaration in view at once), and the LSP spec requires a
//! token to identify one progress while it runs.  A client that tracks
//! progress by token sees the second `begin` of a shared token replace
//! the first, so one of the two never ends.  This drives the real
//! transport, answers the server's `window/workDoneProgress/create`
//! requests like an editor, and checks that every request reports under a
//! token of its own that ends exactly once.

use std::collections::HashMap;
use std::time::Duration;

use crate::common::lsp_transport::serve_backend;

const URI: &str = "file:///shapes.php";

const SOURCE: &str = "<?php
interface Shape
{
    public function area(): float;
}

final class Square implements Shape
{
    public function area(): float
    {
        return 1.0;
    }
}
";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_requests_report_progress_under_their_own_tokens() {
    let mut client = serve_backend();
    client
        .initialize(serde_json::json!({ "window": { "workDoneProgress": true } }))
        .await;
    client
        .send(
            "didOpen",
            serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": URI, "languageId": "php", "version": 1, "text": SOURCE
                }}
            }),
        )
        .await;

    let requests = [
        (2, "textDocument/implementation"),
        (3, "textDocument/implementation"),
        (4, "textDocument/references"),
        (5, "textDocument/references"),
    ];
    for (id, method) in requests {
        client
            .send(
                method,
                serde_json::json!({
                    "jsonrpc": "2.0", "id": id, "method": method,
                    "params": {
                        "textDocument": { "uri": URI },
                        "position": { "line": 3, "character": 21 },
                        "context": { "includeDeclaration": true }
                    }
                }),
            )
            .await;
    }

    let mut begins: Vec<String> = Vec::new();
    let mut ends: HashMap<String, usize> = HashMap::new();
    let mut answered = 0;
    while answered < requests.len() || ends.values().sum::<usize>() < begins.len() {
        let msg = tokio::time::timeout(
            Duration::from_secs(30),
            client.read_until("a response or progress", |_| true),
        )
        .await
        .unwrap_or_else(|_| panic!("a progress never ended: begins {begins:?}, ends {ends:?}"));
        match msg.get("method").and_then(|m| m.as_str()) {
            Some("window/workDoneProgress/create") => {
                let id = msg["id"].clone();
                client
                    .send(
                        "the create response",
                        serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                    )
                    .await;
            }
            Some("$/progress") => {
                let token = msg["params"]["token"].to_string();
                match msg["params"]["value"]["kind"].as_str() {
                    Some("begin") => begins.push(token),
                    Some("end") => *ends.entry(token).or_default() += 1,
                    _ => {}
                }
            }
            Some(_) => {}
            None => answered += 1,
        }
    }

    assert_eq!(
        begins.len(),
        requests.len(),
        "every request reports progress: {begins:?}"
    );
    for token in &begins {
        assert_eq!(
            begins.iter().filter(|t| *t == token).count(),
            1,
            "token {token} began more than once: {begins:?}"
        );
        assert_eq!(
            ends.get(token),
            Some(&1),
            "token {token} ends exactly once: {ends:?}"
        );
    }
}
