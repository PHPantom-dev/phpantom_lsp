//! Driving a [`LanguageServer`] over the real tower-lsp transport.
//!
//! Most suites call a `Backend` handler directly.  A test of the server
//! as the editor sees it (request concurrency, a client that answers
//! late, a trait method that gates on the file kind before any handler
//! runs) needs the JSON-RPC layer in between: the helpers here frame and
//! parse `Content-Length` messages over an in-memory duplex stream and run
//! the [`Server`] with the same concurrency the binary uses.

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tower_lsp::{Client, LanguageServer, LspService, Server};

use phpantom_lsp::{Backend, LSP_CONCURRENCY};

/// The client end of a transport with a language server behind it.
///
/// `buf` keeps the bytes read past the last complete frame, so a response
/// that arrived together with an earlier one is not lost between reads.
pub struct TransportClient {
    stream: DuplexStream,
    buf: Vec<u8>,
}

/// Serve `server` over an in-memory transport, configured the way the
/// binary configures it, and return the client end.
pub fn serve<S: LanguageServer>(build: impl FnOnce(Client) -> S) -> TransportClient {
    let (service, socket) = LspService::build(build).finish();
    let (client, server) = tokio::io::duplex(1 << 16);
    let (server_read, server_write) = tokio::io::split(server);
    tokio::spawn(
        Server::new(server_read, server_write, socket)
            .concurrency_level(LSP_CONCURRENCY)
            .serve(service),
    );
    TransportClient {
        stream: client,
        buf: Vec::new(),
    }
}

/// [`serve`] for the real [`Backend`].
pub fn serve_backend() -> TransportClient {
    serve(Backend::new)
}

/// A [`Backend`] with a client attached, for calling its
/// [`LanguageServer`] methods directly rather than over a transport.
///
/// The returned service owns the backend (`service.inner()`); the socket
/// has to stay alive for as long as the backend may notify the client.
pub fn backend_with_client() -> (LspService<Backend>, tower_lsp::ClientSocket) {
    LspService::build(Backend::new).finish()
}

impl TransportClient {
    /// Send one framed message, naming the server's death if the pipe is
    /// already gone: a serve-loop panic kills the transport, so the next
    /// write is often where a regression first shows up.
    pub async fn send(&mut self, what: &str, value: serde_json::Value) {
        self.stream
            .write_all(&frame(value))
            .await
            .unwrap_or_else(|e| panic!("the server was gone before {what} could be sent: {e}"));
    }

    /// Read framed messages until `pred` accepts one, returning it.
    /// Panics naming `what` if the server closes the stream first, which
    /// is what a serve-loop panic looks like from the client side.
    pub async fn read_until(
        &mut self,
        what: &str,
        mut pred: impl FnMut(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let mut chunk = [0u8; 4096];
        loop {
            while let Some((msg, consumed)) = try_parse_frame(&self.buf) {
                self.buf.drain(..consumed);
                if pred(&msg) {
                    return msg;
                }
            }
            let n =
                self.stream.read(&mut chunk).await.unwrap_or_else(|e| {
                    panic!("server closed the stream before {what} arrived: {e}")
                });
            assert!(n > 0, "server closed the stream before {what} arrived");
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// Read until the response to request `id` arrives, returning how long
    /// that took.
    pub async fn wait_for_id(&mut self, id: i64) -> Duration {
        let start = Instant::now();
        self.read_until(&format!("the response to request {id}"), |msg| {
            msg.get("method").is_none() && msg.get("id").and_then(|v| v.as_i64()) == Some(id)
        })
        .await;
        start.elapsed()
    }

    /// Complete the `initialize` / `initialized` handshake with the given
    /// client capabilities, as request id 1.
    pub async fn initialize(&mut self, capabilities: serde_json::Value) {
        self.send(
            "initialize",
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "capabilities": capabilities }
            }),
        )
        .await;
        self.wait_for_id(1).await;
        self.send(
            "initialized",
            serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
        )
        .await;
    }
}

/// Frame a JSON-RPC message with the LSP `Content-Length` header.
pub fn frame(value: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&value).unwrap();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend_from_slice(&body);
    out
}

/// Try to parse one `Content-Length`-framed JSON message from `buf`,
/// returning it with the number of bytes it occupied.
pub fn try_parse_frame(buf: &[u8]) -> Option<(serde_json::Value, usize)> {
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
