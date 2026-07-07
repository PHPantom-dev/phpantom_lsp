//! Regression test for the per-keystroke request-barrage wedge.
//!
//! Editors fire a large burst of requests on every keystroke (completion, a
//! resolve per visible item, diagnostics, code lens, semantic tokens, …).
//! tower-lsp's default request concurrency is 4, and once that many requests
//! are in flight the transport's task queue fills and the message-read loop
//! blocks — so the server stops responding to *everything*, including the
//! `$/cancelRequest` notifications that would drain the backlog. The user sees
//! completion "respond a few times, then stall and get cancelled."
//!
//! [`phpantom_lsp::LSP_CONCURRENCY`] raises that limit. This test drives the
//! real tower-lsp [`Server`] — configured exactly as the binary configures it
//! — over an in-memory duplex stream with a deliberately slow `completion`
//! handler, floods it with a burst of completions, and asserts that a cheap
//! request sent *after* the burst still comes back promptly instead of queuing
//! behind the slow burst. With the tower-lsp default of 4 this fails (the cheap
//! request waits for several slow rounds); with `LSP_CONCURRENCY` it passes.
//!
//! It is intentionally self-contained: a dummy language server with controlled
//! timing, no dependency on `examples/` or any real workspace.

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result};

/// One slow completion. Long enough that, if completions are processed only a
/// few at a time, a request queued behind a burst of them is visibly delayed.
const COMPLETION_DELAY: Duration = Duration::from_millis(150);

/// A minimal language server: `completion` sleeps, `document_highlight` is
/// instant. Everything else uses the trait defaults.
struct SlowServer;

#[tower_lsp::async_trait]
impl LanguageServer for SlowServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult::default())
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        // `tokio::time::sleep` yields, modelling a handler that occupies an
        // in-flight slot for a while (the real server occupies the slot for the
        // duration of its blocking resolution work).
        tokio::time::sleep(COMPLETION_DELAY).await;
        Ok(None)
    }

    async fn document_highlight(
        &self,
        _: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        Ok(None)
    }
}

/// Frame a JSON-RPC message with the LSP `Content-Length` header.
fn frame(value: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).unwrap();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(&body);
    out
}

/// Read framed messages off the stream until one with `id == wanted` arrives,
/// returning how long that took. Times out via the caller's `tokio::time`.
async fn wait_for_id(stream: &mut DuplexStream, wanted: i64) -> Duration {
    let start = Instant::now();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        // Parse any complete frames already buffered.
        while let Some((msg, consumed)) = try_parse_frame(&buf) {
            buf.drain(..consumed);
            if msg.get("id").and_then(|v| v.as_i64()) == Some(wanted) {
                return start.elapsed();
            }
        }
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "server closed the stream before id {wanted} arrived");
        buf.extend_from_slice(&chunk[..n]);
    }
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

/// A burst of slow completions must not starve a cheap request sent right after
/// them: the cheap request comes back long before the whole burst could drain
/// at low concurrency.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cheap_request_not_starved_by_completion_burst() {
    let uri = "file:///t.php";
    let (service, socket) = LspService::build(|_client: Client| SlowServer).finish();
    let (mut client, server) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server);

    // Drive the real transport with the same concurrency the binary uses.
    tokio::spawn(
        Server::new(server_read, server_write, socket)
            .concurrency_level(phpantom_lsp::LSP_CONCURRENCY)
            .serve(service),
    );

    // Initialize.
    client
        .write_all(&frame(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"capabilities": {}}
        })))
        .await
        .unwrap();
    wait_for_id(&mut client, 1).await;
    client
        .write_all(&frame(serde_json::json!({
            "jsonrpc": "2.0", "method": "initialized", "params": {}
        })))
        .await
        .unwrap();

    // Fire a burst of slow completions WITHOUT reading their responses, the way
    // an editor pipelines a keystroke barrage.
    const BURST: i64 = 40;
    for id in 100..100 + BURST {
        client
            .write_all(&frame(serde_json::json!({
                "jsonrpc": "2.0", "id": id, "method": "textDocument/completion",
                "params": {
                    "textDocument": {"uri": uri},
                    "position": {"line": 0, "character": 0}
                }
            })))
            .await
            .unwrap();
    }

    // Then a cheap request. It must not wait for the whole burst to drain.
    let cheap_id = 9999;
    client
        .write_all(&frame(serde_json::json!({
            "jsonrpc": "2.0", "id": cheap_id, "method": "textDocument/documentHighlight",
            "params": {
                "textDocument": {"uri": uri},
                "position": {"line": 0, "character": 0}
            }
        })))
        .await
        .unwrap();

    let latency = tokio::time::timeout(Duration::from_secs(5), wait_for_id(&mut client, cheap_id))
        .await
        .expect("server wedged: cheap request did not return within 5s under a completion burst");

    // With tower-lsp's default concurrency of 4, the cheap request would queue
    // behind the burst and take roughly (BURST / 4) * COMPLETION_DELAY ≈ 1.5s.
    // With LSP_CONCURRENCY the whole burst is in flight at once, so the cheap
    // request returns almost immediately. Use a threshold that cleanly
    // separates the two regimes without being flaky on slow CI.
    assert!(
        latency < COMPLETION_DELAY * 3,
        "cheap request was starved by the completion burst ({latency:?}); \
         LSP request concurrency is too low"
    );
}
