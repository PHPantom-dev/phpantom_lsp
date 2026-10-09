//! Regression tests for a client that does not answer
//! `client/registerCapability`.
//!
//! `initialized` registers the `workspace/didChangeWatchedFiles` watchers
//! with a server-to-client *request*.  A client that never answers it left
//! the `initialized` handler waiting, and the `textDocument/didOpen` calls
//! that follow were not handled while it did, so no file was diagnosed.
//! Completion, hover and the other requests kept answering, which hid the
//! cause.  The server also sent the request to clients that never said they
//! accept dynamic registration of watchers, which is how it reached a
//! client that does not answer it.
//!
//! These drive the real [`phpantom_lsp::Backend`] over the real tower-lsp
//! transport, exactly as the binary wires it up, with a client that records
//! what the server asks of it and never replies.

use std::time::Duration;

use crate::common::lsp_transport::{TransportClient, serve_backend};

const URI: &str = "file:///t.php";

/// How long the server gets to produce what a test waits for.  Generous,
/// because a debug build diagnosing its first file also loads the stubs;
/// a server that is stuck never produces it at all.
const PATIENCE: Duration = Duration::from_secs(30);

/// The method of a server-to-client message, if it carries one.
fn method(msg: &serde_json::Value) -> Option<&str> {
    msg.get("method").and_then(|m| m.as_str())
}

/// Open a file whose unused import is diagnosed without resolving anything.
async fn open_file_with_an_unused_import(client: &mut TransportClient) {
    client
        .send(
            "didOpen",
            serde_json::json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": URI, "languageId": "php", "version": 1,
                    "text": "<?php\nuse Foo\\Bar;\n"
                }}
            }),
        )
        .await;
}

/// Read until the diagnostics for [`URI`] are published, returning every
/// method the server sent on the way.
async fn methods_until_diagnostics_are_published(
    client: &mut TransportClient,
    why: &str,
) -> Vec<String> {
    let mut seen = Vec::new();
    tokio::time::timeout(
        PATIENCE,
        client.read_until("the diagnostics for the opened file", |msg| {
            let Some(method) = method(msg) else {
                return false;
            };
            seen.push(method.to_string());
            method == "textDocument/publishDiagnostics"
                && msg["params"]["uri"] == URI
                && msg["params"]["diagnostics"]
                    .as_array()
                    .is_some_and(|diagnostics| !diagnostics.is_empty())
        }),
    )
    .await
    .unwrap_or_else(|_| panic!("{why}; the server sent {seen:?}"));
    seen
}

/// The methods of the registrations in a `client/registerCapability`
/// request.
fn registered_methods(request: &serde_json::Value) -> Vec<&str> {
    request["params"]["registrations"]
        .as_array()
        .expect("a registration request lists its registrations")
        .iter()
        .filter_map(|registration| registration["method"].as_str())
        .collect()
}

/// A client that never answers the watcher registration must not stop the
/// files it opens from being diagnosed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unanswered_watcher_registration_does_not_hold_back_diagnostics() {
    let mut client = serve_backend();
    client
        .initialize(serde_json::json!({
            "workspace": { "didChangeWatchedFiles": { "dynamicRegistration": true } }
        }))
        .await;

    let registration = tokio::time::timeout(
        PATIENCE,
        client.read_until("the watcher registration", |msg| {
            method(msg) == Some("client/registerCapability")
        }),
    )
    .await
    .expect("a client that accepts dynamic registration is asked to register its watchers");
    assert_eq!(
        registered_methods(&registration),
        ["workspace/didChangeWatchedFiles"]
    );

    // The registration stays unanswered from here on.
    open_file_with_an_unused_import(&mut client).await;
    methods_until_diagnostics_are_published(
        &mut client,
        "an unanswered registration held back the diagnostics",
    )
    .await;
}

/// A client that did not say it accepts dynamic registration of watchers
/// is not asked to register any, but still gets the other registrations
/// it asked for.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn watchers_are_left_out_for_a_client_that_did_not_opt_in() {
    let mut client = serve_backend();
    client
        .initialize(serde_json::json!({
            "textDocument": { "typeHierarchy": { "dynamicRegistration": true } }
        }))
        .await;

    let registration = tokio::time::timeout(
        PATIENCE,
        client.read_until("the type hierarchy registration", |msg| {
            method(msg) == Some("client/registerCapability")
        }),
    )
    .await
    .expect("a client that accepts dynamic registration of type hierarchy is asked for it");
    assert_eq!(
        registered_methods(&registration),
        ["textDocument/prepareTypeHierarchy"]
    );

    // The registration stays unanswered here too, and the file is still
    // diagnosed.
    open_file_with_an_unused_import(&mut client).await;
    methods_until_diagnostics_are_published(
        &mut client,
        "an unanswered registration held back the diagnostics",
    )
    .await;
}

/// A client that accepts no dynamic registration at all is sent no
/// registration request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_that_accepts_no_dynamic_registration_is_asked_for_none() {
    let mut client = serve_backend();
    client.initialize(serde_json::json!({})).await;

    open_file_with_an_unused_import(&mut client).await;
    let seen = methods_until_diagnostics_are_published(
        &mut client,
        "a client that cannot register anything held back the diagnostics",
    )
    .await;

    assert!(
        !seen.iter().any(|m| m == "client/registerCapability"),
        "no registration should be requested, the server sent {seen:?}"
    );
}
