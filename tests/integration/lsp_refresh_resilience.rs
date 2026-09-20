//! Regression test for the late `workspace/diagnostic/refresh` response
//! panic.
//!
//! `workspace/diagnostic/refresh` is a server-to-client *request*.  In
//! tower-lsp, the future returned by `Client::workspace_diagnostic_refresh`
//! owns the response channel's receiver; if that future is dropped — which
//! is what wrapping it in `tokio::time::timeout` does when the client is
//! slow — the response that eventually arrives finds the receiver gone and
//! `Pending::insert` panics with "receiver already dropped" *on the serve
//! loop itself*, killing the whole server.  An editor that is busy for ten
//! seconds (indexing burst, GC pause, its own plugins) and then answers is
//! all it takes; users saw go-to-definition hang forever because the
//! process behind it was dead ([#437]).
//!
//! [#437]: https://github.com/PHPantom-dev/phpantom_lsp/issues/437
//!
//! This drives the real [`phpantom_lsp::Backend`] over the real tower-lsp
//! transport, exactly as the binary wires it up: initialize in pull mode,
//! open and close a file so the server issues a refresh, answer that
//! refresh only after a delay longer than the timeout the server used to
//! race it against, and assert the server still answers an ordinary
//! request afterwards.  Against the timeout-wrapping code this dies on the
//! panic (the stream closes); with the refresh pump it passes.

use std::time::Duration;

use crate::common::lsp_transport::serve_backend;

/// How long the client withholds its refresh response.  The hazard window
/// opened once the server abandoned the request, which happened after the
/// 10-second timeout the old code raced the request against, so the delay
/// must outlast that.
const RESPONSE_DELAY: Duration = Duration::from_secs(11);

/// A refresh response arriving long after the server stopped waiting for
/// it must not bring the server down.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_refresh_response_does_not_kill_the_server() {
    let mut client = serve_backend();

    // Initialize in pull-diagnostic mode (the `diagnostic` capability is
    // what makes the server ask for refreshes) with no workspace, so no
    // indexing competes with the test.
    client
        .initialize(serde_json::json!({ "textDocument": { "diagnostic": {} } }))
        .await;

    // Opening and closing a file makes the server request a diagnostic
    // refresh (closing always does in pull mode, to clear the file's
    // entry).
    let did_open = serde_json::json!({
        "jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": { "textDocument": {
            "uri": "file:///t.php", "languageId": "php", "version": 1,
            "text": "<?php\nuse Foo\\Bar;\n"
        }}
    });
    client.send("didOpen", did_open).await;
    let did_close = serde_json::json!({
        "jsonrpc": "2.0", "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": "file:///t.php" } }
    });
    client.send("didClose", did_close).await;

    let refresh = tokio::time::timeout(
        Duration::from_secs(30),
        client.read_until("the refresh request", |msg| {
            msg.get("method").and_then(|m| m.as_str()) == Some("workspace/diagnostic/refresh")
        }),
    )
    .await
    .expect("the server should request a diagnostic refresh after didClose");
    let refresh_id = refresh.get("id").cloned().expect("a request carries an id");

    // Answer only once the server would long since have abandoned the
    // request, were it racing it against a timeout.
    tokio::time::sleep(RESPONSE_DELAY).await;
    let response = serde_json::json!({
        "jsonrpc": "2.0", "id": refresh_id, "result": null
    });
    client.send("the refresh response", response).await;

    // Give a would-be panic time to tear the serve loop down, then check
    // the server is still there: an ordinary request must get an answer.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
    });
    client.send("shutdown", shutdown).await;
    tokio::time::timeout(Duration::from_secs(10), client.wait_for_id(99))
        .await
        .expect("the server should still answer after the late refresh response");
}
