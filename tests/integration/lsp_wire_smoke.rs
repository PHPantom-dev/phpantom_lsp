//! One round trip per advertised capability over the real JSON-RPC
//! transport.
//!
//! Every other suite calls a `Backend` handler directly, which skips the
//! protocol layer: request routing, the shape of a response once it is
//! serialized, and how positions are encoded on the wire.  These tests
//! drive the server the way an editor does, through
//! [`serve_backend`], and check only what that layer decides.  What the
//! answers mean is covered by the feature suites.
//!
//! The document puts its symbols after an emoji, which is two UTF-16 code
//! units, one `char` and four bytes, so a position counted in anything but
//! UTF-16 lands on the wrong character.

use std::time::Duration;

use serde_json::{Value, json};

use crate::common::lsp_transport::{TransportClient, serve_backend};

const URI: &str = "file:///smoke.php";

/// How long a request gets to answer.  Generous, because the first request
/// in a debug build also loads the stubs.
const PATIENCE: Duration = Duration::from_secs(30);

const SOURCE: &str = "<?php
namespace App;

interface Greets {}

class Greeter implements Greets {
    /** Says hello. */
    public function hello(string $name): string { return $name; }
}

function run(Greeter $g): void {
    $s = '😀'; $g->hello($s);
}

function caller(): void { run(new Greeter()); }
";

/// The line of `SOURCE` holding the call after the emoji.
const CALL_LINE: u32 = 11;

/// The UTF-16 column of the first `needle` on `line` of `SOURCE`.
fn column(line: u32, needle: &str) -> u32 {
    let text = SOURCE.lines().nth(line as usize).unwrap();
    let byte = text
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` is not on line {line}"));
    text[..byte].encode_utf16().count() as u32
}

/// The position of the first `needle` on `line`.
fn at(line: u32, needle: &str) -> Value {
    json!({ "line": line, "character": column(line, needle) })
}

fn doc_position(line: u32, needle: &str) -> Value {
    json!({ "textDocument": { "uri": URI }, "position": at(line, needle) })
}

/// A range as it appears on the wire, for comparing against a response.
fn range(line: u32, needle: &str) -> Value {
    let start = column(line, needle);
    let end = start + needle.encode_utf16().count() as u32;
    json!({
        "start": { "line": line, "character": start },
        "end": { "line": line, "character": end },
    })
}

struct Session {
    client: TransportClient,
    next_id: i64,
}

impl Session {
    /// Serve a backend, initialize it as a client that pulls diagnostics,
    /// and open [`SOURCE`].
    async fn open() -> Self {
        let mut client = serve_backend();
        client
            .initialize(json!({
                "textDocument": { "diagnostic": { "dynamicRegistration": false } }
            }))
            .await;
        client
            .send(
                "didOpen",
                json!({
                    "jsonrpc": "2.0", "method": "textDocument/didOpen",
                    "params": { "textDocument": {
                        "uri": URI, "languageId": "php", "version": 1, "text": SOURCE
                    }}
                }),
            )
            .await;
        Session {
            client,
            next_id: 100,
        }
    }

    /// Send a request and return its whole response message.
    async fn raw(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.client
            .send(
                method,
                json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
            )
            .await;
        tokio::time::timeout(
            PATIENCE,
            self.client
                .read_until(&format!("the response to {method}"), |msg| {
                    msg.get("method").is_none() && msg["id"] == id
                }),
        )
        .await
        .unwrap_or_else(|_| panic!("{method} did not answer"))
    }

    /// Send a request and return its result, failing on an error response.
    async fn call(&mut self, method: &str, params: Value) -> Value {
        let response = self.raw(method, params).await;
        assert!(
            response.get("error").is_none(),
            "{method} answered with an error: {response}"
        );
        response
            .get("result")
            .cloned()
            .unwrap_or_else(|| panic!("{method} answered without a result: {response}"))
    }

    /// [`Self::call`], failing on a `null` result too.
    async fn call_some(&mut self, method: &str, params: Value) -> Value {
        let result = self.call(method, params).await;
        assert!(!result.is_null(), "{method} answered null");
        result
    }
}

/// Every location in a `Location[]` / `LocationLink[]` result, as `(uri,
/// range)`.
fn locations(result: &Value) -> Vec<(Value, Value)> {
    let list = match result {
        Value::Array(list) => list.clone(),
        Value::Null => Vec::new(),
        single => vec![single.clone()],
    };
    list.iter()
        .map(|l| match l.get("targetUri") {
            Some(uri) => (uri.clone(), l["targetSelectionRange"].clone()),
            None => (l["uri"].clone(), l["range"].clone()),
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn navigation_requests_encode_positions_in_utf16() {
    let mut s = Session::open().await;
    let declaration = range(7, "hello");
    let call = range(CALL_LINE, "hello");

    let hover = s
        .call_some("textDocument/hover", doc_position(CALL_LINE, "hello"))
        .await;
    assert!(
        hover["contents"]["value"]
            .as_str()
            .is_some_and(|text| text.contains("Says hello")),
        "{hover}"
    );

    // Definitions point at where the name starts.
    for method in ["textDocument/definition", "textDocument/declaration"] {
        let result = s.call_some(method, doc_position(CALL_LINE, "hello")).await;
        assert_eq!(
            locations(&result)
                .iter()
                .map(|(uri, r)| (uri.clone(), r["start"].clone()))
                .collect::<Vec<_>>(),
            [(json!(URI), declaration["start"].clone())],
            "{method}"
        );
    }

    let type_definition = s
        .call_some("textDocument/typeDefinition", doc_position(CALL_LINE, "$g"))
        .await;
    assert_eq!(
        locations(&type_definition)
            .iter()
            .map(|(_, r)| r["start"]["line"].clone())
            .collect::<Vec<_>>(),
        [json!(5)],
        "{type_definition}"
    );

    let implementation = s
        .call_some("textDocument/implementation", doc_position(3, "Greets"))
        .await;
    assert_eq!(
        locations(&implementation)
            .iter()
            .map(|(_, r)| r["start"]["line"].clone())
            .collect::<Vec<_>>(),
        [json!(5)],
        "{implementation}"
    );

    let mut params = doc_position(CALL_LINE, "hello");
    params["context"] = json!({ "includeDeclaration": true });
    let references = s.call_some("textDocument/references", params).await;
    let mut found: Vec<Value> = locations(&references).into_iter().map(|(_, r)| r).collect();
    found.sort_by_key(|r| r["start"]["line"].as_u64());
    assert_eq!(found, [declaration.clone(), call.clone()]);

    let highlights = s
        .call_some(
            "textDocument/documentHighlight",
            doc_position(CALL_LINE, "hello"),
        )
        .await;
    let mut found: Vec<Value> = highlights
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["range"].clone())
        .collect();
    found.sort_by_key(|r| r["start"]["line"].as_u64());
    assert_eq!(found, [declaration, call]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn editing_requests_round_trip() {
    let mut s = Session::open().await;

    // Completion right after `->`, behind the emoji, then resolve an item.
    let mut position = at(CALL_LINE, "hello");
    let completion = s
        .call_some(
            "textDocument/completion",
            json!({ "textDocument": { "uri": URI }, "position": position.clone() }),
        )
        .await;
    let items = completion
        .get("items")
        .unwrap_or(&completion)
        .as_array()
        .unwrap()
        .clone();
    let item = items
        .iter()
        .find(|i| i["label"].as_str().is_some_and(|l| l.starts_with("hello")))
        .unwrap_or_else(|| panic!("no `hello` completion: {completion}"))
        .clone();
    let resolved = s.call_some("completionItem/resolve", item).await;
    assert!(
        resolved["label"]
            .as_str()
            .is_some_and(|l| l.starts_with("hello"))
    );

    // Inside the call's parentheses.
    position["character"] = json!(column(CALL_LINE, "$s)"));
    let help = s
        .call_some(
            "textDocument/signatureHelp",
            json!({ "textDocument": { "uri": URI }, "position": position }),
        )
        .await;
    assert!(
        help["signatures"][0]["label"]
            .as_str()
            .is_some_and(|l| l.contains("string $name")),
        "{help}"
    );

    let prepared = s
        .call_some(
            "textDocument/prepareRename",
            doc_position(CALL_LINE, "hello"),
        )
        .await;
    let prepared_range = prepared.get("range").unwrap_or(&prepared);
    assert_eq!(*prepared_range, range(CALL_LINE, "hello"), "{prepared}");

    let mut params = doc_position(CALL_LINE, "hello");
    params["newName"] = json!("greet");
    let rename = s.call_some("textDocument/rename", params).await;
    let edits = rename["changes"][URI]
        .as_array()
        .cloned()
        .unwrap_or_else(|| {
            rename["documentChanges"][0]["edits"]
                .as_array()
                .cloned()
                .unwrap_or_else(|| panic!("no edits for the file: {rename}"))
        });
    let mut ranges: Vec<Value> = edits.iter().map(|e| e["range"].clone()).collect();
    ranges.sort_by_key(|r| r["start"]["line"].as_u64());
    assert_eq!(ranges, [range(7, "hello"), range(CALL_LINE, "hello")]);

    let whole = json!({
        "start": { "line": 0, "character": 0 },
        "end": { "line": SOURCE.lines().count(), "character": 0 },
    });
    s.call(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": URI }, "range": whole.clone(),
            "context": { "diagnostics": [] }
        }),
    )
    .await;
    let options = json!({ "tabSize": 4, "insertSpaces": true });
    s.call(
        "textDocument/formatting",
        json!({ "textDocument": { "uri": URI }, "options": options.clone() }),
    )
    .await;
    s.call(
        "textDocument/rangeFormatting",
        json!({ "textDocument": { "uri": URI }, "range": whole, "options": options.clone() }),
    )
    .await;
    s.call(
        "textDocument/onTypeFormatting",
        json!({
            "textDocument": { "uri": URI }, "position": { "line": 9, "character": 0 },
            "ch": "\n", "options": options
        }),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn document_requests_round_trip() {
    let mut s = Session::open().await;
    let doc = json!({ "textDocument": { "uri": URI } });
    let whole = json!({
        "start": { "line": 0, "character": 0 },
        "end": { "line": SOURCE.lines().count(), "character": 0 },
    });

    let symbols = s
        .call_some("textDocument/documentSymbol", doc.clone())
        .await;
    assert!(symbols.to_string().contains("Greeter"), "{symbols}");

    let workspace = s
        .call_some("workspace/symbol", json!({ "query": "Greeter" }))
        .await;
    assert!(workspace.to_string().contains("Greeter"), "{workspace}");

    let folding = s.call_some("textDocument/foldingRange", doc.clone()).await;
    assert!(!folding.as_array().unwrap().is_empty());

    let selection = s
        .call_some(
            "textDocument/selectionRange",
            json!({ "textDocument": { "uri": URI }, "positions": [at(CALL_LINE, "hello")] }),
        )
        .await;
    assert_eq!(selection[0]["range"], range(CALL_LINE, "hello"));

    let tokens = s
        .call_some("textDocument/semanticTokens/full", doc.clone())
        .await;
    let data = tokens["data"].as_array().unwrap();
    assert_eq!(data.len() % 5, 0, "tokens come in fives");
    // Each token is (delta line, delta start, length, type, modifiers);
    // find the one for `$g` behind the emoji.
    let (mut line, mut start) = (0u64, 0u64);
    let mut hit = false;
    for token in data.chunks(5) {
        let delta_line = token[0].as_u64().unwrap();
        let delta_start = token[1].as_u64().unwrap();
        if delta_line > 0 {
            line += delta_line;
            start = delta_start;
        } else {
            start += delta_start;
        }
        if line == u64::from(CALL_LINE) && start == u64::from(column(CALL_LINE, "$g")) {
            assert_eq!(token[2], json!(2), "the token is `$g` in UTF-16 units");
            hit = true;
        }
    }
    assert!(hit, "no token at the `$g` behind the emoji: {tokens}");

    s.call(
        "textDocument/inlayHint",
        json!({ "textDocument": { "uri": URI }, "range": whole }),
    )
    .await;
    let lenses = s.call("textDocument/codeLens", doc.clone()).await;
    if let Some(lens) = lenses.as_array().and_then(|l| l.first()).cloned() {
        s.call_some("codeLens/resolve", lens).await;
    }
    s.call("textDocument/documentLink", doc.clone()).await;

    let items = s
        .call_some("textDocument/prepareCallHierarchy", doc_position(10, "run"))
        .await;
    let incoming = s
        .call_some(
            "callHierarchy/incomingCalls",
            json!({ "item": items[0].clone() }),
        )
        .await;
    assert_eq!(incoming[0]["from"]["name"], json!("caller"), "{incoming}");

    let items = s
        .call_some(
            "textDocument/prepareTypeHierarchy",
            doc_position(5, "Greeter"),
        )
        .await;
    let supertypes = s
        .call_some(
            "typeHierarchy/supertypes",
            json!({ "item": items[0].clone() }),
        )
        .await;
    assert!(supertypes.to_string().contains("Greets"), "{supertypes}");
}

/// An incremental edit placed by UTF-16 columns lands where the client
/// meant, and the diagnostic it causes comes back at UTF-16 columns.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn incremental_edits_and_pulled_diagnostics_use_utf16() {
    let mut s = Session::open().await;
    let doc = json!({ "textDocument": { "uri": URI } });

    let report = s.call_some("textDocument/diagnostic", doc.clone()).await;
    assert_eq!(report["kind"], json!("full"));
    assert_eq!(report["items"], json!([]), "the file starts clean");

    s.client
        .send(
            "didChange",
            json!({
                "jsonrpc": "2.0", "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": URI, "version": 2 },
                    "contentChanges": [{
                        "range": range(CALL_LINE, "hello"),
                        "text": "hallo"
                    }]
                }
            }),
        )
        .await;

    let report = s.call_some("textDocument/diagnostic", doc).await;
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{report}");
    assert_eq!(items[0]["code"], json!("unknown_member"), "{report}");
    assert_eq!(items[0]["range"], range(CALL_LINE, "hello"), "{report}");

    let workspace = s
        .call_some("workspace/diagnostic", json!({ "previousResultIds": [] }))
        .await;
    assert!(workspace["items"].is_array(), "{workspace}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn protocol_errors_and_shutdown() {
    let mut s = Session::open().await;

    let unknown = s.raw("textDocument/noSuchMethod", json!({})).await;
    assert_eq!(unknown["error"]["code"], json!(-32601), "{unknown}");

    // A request for a file that was never opened answers rather than
    // failing the transport.
    let mut params = doc_position(CALL_LINE, "hello");
    params["textDocument"]["uri"] = json!("file:///never-opened.php");
    s.raw("textDocument/hover", params).await;

    assert_eq!(s.call("shutdown", Value::Null).await, Value::Null);
    let after = s
        .raw("textDocument/hover", doc_position(CALL_LINE, "hello"))
        .await;
    assert_eq!(after["error"]["code"], json!(-32600), "{after}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_before_initialize_are_refused() {
    let mut client = serve_backend();
    client
        .send(
            "hover",
            json!({
                "jsonrpc": "2.0", "id": 7, "method": "textDocument/hover",
                "params": doc_position(CALL_LINE, "hello")
            }),
        )
        .await;
    let response = tokio::time::timeout(
        PATIENCE,
        client.read_until("the refusal", |msg| msg["id"] == 7),
    )
    .await
    .expect("the server answers a request sent before initialize");
    assert_eq!(response["error"]["code"], json!(-32002), "{response}");
}
