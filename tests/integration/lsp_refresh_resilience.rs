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

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tower_lsp::{LspService, Server};

use phpantom_lsp::{Backend, LSP_CONCURRENCY};

/// How long the client withholds its refresh response.  The hazard window
/// opened once the server abandoned the request, which happened after the
/// 10-second timeout the old code raced the request against, so the delay
/// must outlast that.
const RESPONSE_DELAY: Duration = Duration::from_secs(11);

/// Frame a JSON-RPC message with the LSP `Content-Length` header.
fn frame(value: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).unwrap();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(&body);
    out
}

/// Try to parse one `Content-Length`-framed JSON message from `buf`.
fn try_parse_frame(buf: &[u8]) -> Option<(serde_json::Value, usize)> {
    let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header = std::str::from_utf8(&buf[..header_end]).ok()?;
    let len: usize = header
        .lines()
        .find_map(|l| l.strip_prefix("Content-Length: "))?
        .trim()
        .parse()
        .ok()?;
    let body_start = header_end + 4;
    let body_end = body_start + len;
    if buf.len() < body_end {
        return None;
    }
    let value = serde_json::from_slice(&buf[body_start..body_end]).ok()?;
    Some((value, body_end))
}

/// Read framed messages until `pred` accepts one, returning it.  Panics
/// with `what` if the server closes the stream first — which is exactly
/// what the serve-loop panic under test looks like from the client side.
async fn read_until(
    stream: &mut DuplexStream,
    buf: &mut Vec<u8>,
    what: &str,
    mut pred: impl FnMut(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let mut chunk = [0u8; 4096];
    loop {
        while let Some((msg, consumed)) = try_parse_frame(buf) {
            buf.drain(..consumed);
            if pred(&msg) {
                return msg;
            }
        }
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "server closed the stream before {what} arrived");
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// A refresh response arriving long after the server stopped waiting for
/// it must not bring the server down.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_refresh_response_does_not_kill_the_server() {
    let (service, socket) = LspService::build(Backend::new).finish();
    let (mut client, server) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server);
    tokio::spawn(
        Server::new(server_read, server_write, socket)
            .concurrency_level(LSP_CONCURRENCY)
            .serve(service),
    );
    let mut buf: Vec<u8> = Vec::new();

    // Initialize in pull-diagnostic mode (the `diagnostic` capability is
    // what makes the server ask for refreshes) with no workspace, so no
    // indexing competes with the test.
    let init = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "capabilities": { "textDocument": { "diagnostic": {} } }
        }
    });
    client.write_all(&frame(init)).await.unwrap();
    read_until(&mut client, &mut buf, "the initialize response", |msg| {
        msg.get("id").and_then(|v| v.as_i64()) == Some(1)
    })
    .await;
    let initialized = serde_json::json!({
        "jsonrpc": "2.0", "method": "initialized", "params": {}
    });
    client.write_all(&frame(initialized)).await.unwrap();

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
    client.write_all(&frame(did_open)).await.unwrap();
    let did_close = serde_json::json!({
        "jsonrpc": "2.0", "method": "textDocument/didClose",
        "params": { "textDocument": { "uri": "file:///t.php" } }
    });
    client.write_all(&frame(did_close)).await.unwrap();

    let refresh = tokio::time::timeout(
        Duration::from_secs(30),
        read_until(&mut client, &mut buf, "the refresh request", |msg| {
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
    client.write_all(&frame(response)).await.unwrap();

    // Give a would-be panic time to tear the serve loop down, then check
    // the server is still there: an ordinary request must get an answer.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0", "id": 99, "method": "shutdown", "params": null
    });
    client.write_all(&frame(shutdown)).await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(10),
        read_until(&mut client, &mut buf, "the shutdown response", |msg| {
            msg.get("method").is_none() && msg.get("id").and_then(|v| v.as_i64()) == Some(99)
        }),
    )
    .await
    .expect("the server should still answer after the late refresh response");
}
