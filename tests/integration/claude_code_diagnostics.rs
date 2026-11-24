//! Diagnostics delivery to Claude Code, which shows its agent the union of
//! every set published since the agent's last action instead of letting a
//! publish replace the one before it.
//!
//! Drives the real [`phpantom_lsp::Backend`] over the real tower-lsp
//! transport.

use std::time::Duration;

use crate::common::lsp_transport::{TransportClient, serve_backend};

const URI: &str = "file:///edited.php";

/// How long the server gets to publish.  Generous, because a debug build
/// diagnosing its first file also loads the stubs.
const PATIENCE: Duration = Duration::from_secs(30);

/// Read until a publish for [`URI`] carries a diagnostic mentioning
/// `needle`, returning the messages of every publish for it on the way.
async fn publishes_until(client: &mut TransportClient, needle: &str) -> Vec<Vec<String>> {
    let mut publishes = Vec::new();
    tokio::time::timeout(
        PATIENCE,
        client.read_until("the expected diagnostic", |msg| {
            if msg.get("method").and_then(|m| m.as_str()) != Some("textDocument/publishDiagnostics")
                || msg["params"]["uri"] != URI
            {
                return false;
            }
            let messages: Vec<String> = msg["params"]["diagnostics"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d["message"].as_str().map(str::to_owned))
                .collect();
            let found = messages.iter().any(|m| m.contains(needle));
            publishes.push(messages);
            found
        }),
    )
    .await
    .unwrap_or_else(|_| panic!("no diagnostic mentioning {needle:?}; saw {publishes:?}"));
    publishes
}

/// After an edit that removes the unknown class, no publish may still name
/// it: an editor's Phase 1 snapshot does, until the slow pass replaces it,
/// but Claude Code would keep it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_publish_after_an_edit_carries_the_previous_slow_results() {
    let mut client = serve_backend();
    client
        .initialize_with(serde_json::json!({
            "capabilities": {},
            "clientInfo": { "name": "Claude Code", "version": "2.1.0" }
        }))
        .await;

    client
        .send(
            "didOpen",
            serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": URI, "languageId": "php", "version": 1,
                    "text": "<?php\nnew MissingThing();\n"
                }}
            }),
        )
        .await;
    publishes_until(&mut client, "MissingThing").await;

    client
        .send(
            "didChange",
            serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": URI, "version": 2 },
                    "contentChanges": [{ "text": "<?php\nuse Foo\\Bar;\n" }]
                }
            }),
        )
        .await;
    let publishes = publishes_until(&mut client, "Bar").await;

    assert!(
        publishes
            .iter()
            .flatten()
            .all(|message| !message.contains("MissingThing")),
        "a publish after the edit carried the previous text's result: {publishes:?}"
    );
}
