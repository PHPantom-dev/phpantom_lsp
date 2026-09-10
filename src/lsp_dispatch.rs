//! Target-independent LSP JSON-RPC dispatcher for the wasm build.
//!
//! Speaks the LSP protocol so a browser editor can drive PHPantom with
//! `@codemirror/lsp-client` (completion, hover, signature help, …). Routes each
//! request to PHPantom's existing *synchronous* `handle_*` methods, so there is
//! no async runtime and no thread pool: the whole dispatcher runs on the single
//! wasm thread.
//!
//! The WASI reactor exports in [`crate::wasm_wasi`] wrap this.

use serde_json::{Value, json};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentHighlightParams, GotoDefinitionParams, HoverParams,
    RenameParams, SignatureHelpParams, Url,
};

use crate::Backend;
use std::sync::Arc;

/// JSON-RPC error codes we return, from the LSP specification.
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// The result of dispatching one message: a JSON-RPC result, a JSON-RPC error,
/// or nothing at all (a notification).
type Outcome = Option<Result<Value, (i64, String)>>;

pub struct LspDispatcher {
    backend: Backend,
}

impl Default for LspDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl LspDispatcher {
    pub fn new() -> LspDispatcher {
        LspDispatcher {
            backend: Backend::new_headless(),
        }
    }

    /// Handle one LSP JSON-RPC message. Returns a JSON-RPC response string for
    /// requests, or `None` for notifications (which produce no reply).
    pub fn handle(&self, message: &str) -> Option<String> {
        let value: Value = serde_json::from_str(message).ok()?;
        let id = value.get("id").cloned();
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        let params = value.get("params").cloned().unwrap_or(Value::Null);

        let outcome: Outcome = match method {
            "initialize" => Some(Ok(self.capabilities())),
            "shutdown" => Some(Ok(Value::Null)),
            "initialized" | "exit" | "$/cancelRequest" => None,
            "textDocument/didOpen" => {
                self.did_open(params);
                None
            }
            "textDocument/didChange" => {
                self.did_change(params);
                None
            }
            "textDocument/didClose" => {
                self.did_close(params);
                None
            }
            "textDocument/completion" => Some(self.completion(params)),
            "completionItem/resolve" => Some(self.resolve(params)),
            "textDocument/hover" => Some(self.hover(params)),
            "textDocument/definition" => Some(self.definition(params)),
            "textDocument/documentHighlight" => Some(self.document_highlight(params)),
            "textDocument/rename" => Some(self.rename(params)),
            "textDocument/signatureHelp" => Some(self.signature_help(params)),
            _ => {
                // Unknown requests need an error reply; unknown notifications
                // are ignored, as the specification requires.
                if id.is_some() {
                    Some(Err((
                        METHOD_NOT_FOUND,
                        format!("method not found: {method}"),
                    )))
                } else {
                    None
                }
            }
        };

        match (id, outcome) {
            (Some(id), Some(Ok(result))) => {
                Some(json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string())
            }
            (Some(id), Some(Err((code, message)))) => Some(
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
                    .to_string(),
            ),
            _ => None,
        }
    }

    /// Mirror the server's `did_open`: store the buffer (the handlers read it
    /// back through `get_file_content`) and parse/index it.
    fn store(&self, uri: String, text: String) {
        self.backend
            .open_files()
            .write()
            .insert(uri.clone(), Arc::new(text.clone()));
        self.backend.update_ast(&uri, &text);
    }

    fn did_open(&self, params: Value) {
        if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(params) {
            self.store(p.text_document.uri.to_string(), p.text_document.text);
        }
    }

    fn did_change(&self, params: Value) {
        if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(params) {
            // We advertise full sync, so the last change carries the whole
            // document.
            if let Some(change) = p.content_changes.into_iter().next_back() {
                self.store(p.text_document.uri.to_string(), change.text);
            }
        }
    }

    fn did_close(&self, params: Value) {
        if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(params) {
            self.backend
                .open_files()
                .write()
                .remove(&p.text_document.uri.to_string());
        }
    }

    fn completion(&self, params: Value) -> Result<Value, (i64, String)> {
        let params: CompletionParams = serde_json::from_value(params).map_err(bad_params)?;
        match self.backend.handle_completion(params) {
            Ok(Some(response)) => Ok(to_value(response)),
            _ => Ok(Value::Null),
        }
    }

    fn resolve(&self, params: Value) -> Result<Value, (i64, String)> {
        let item: CompletionItem = serde_json::from_value(params).map_err(bad_params)?;
        Ok(to_value(self.backend.handle_completion_resolve(item)))
    }

    fn hover(&self, params: Value) -> Result<Value, (i64, String)> {
        let params: HoverParams = serde_json::from_value(params).map_err(bad_params)?;
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let Some((uri, content)) = self.buffer(&uri) else {
            return Ok(Value::Null);
        };
        Ok(self
            .backend
            .handle_hover(&uri, &content, position)
            .map_or(Value::Null, to_value))
    }

    fn definition(&self, params: Value) -> Result<Value, (i64, String)> {
        let params: GotoDefinitionParams = serde_json::from_value(params).map_err(bad_params)?;
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let Some((uri, content)) = self.buffer(&uri) else {
            return Ok(Value::Null);
        };
        let locations: Vec<_> = self
            .backend
            .resolve_definition(&uri, &content, position)
            .into_iter()
            .map(|location| self.backend.translate_location(location))
            .collect();
        if locations.is_empty() {
            return Ok(Value::Null);
        }
        Ok(to_value(locations))
    }

    fn document_highlight(&self, params: Value) -> Result<Value, (i64, String)> {
        let params: DocumentHighlightParams = serde_json::from_value(params).map_err(bad_params)?;
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let Some((uri, content)) = self.buffer(&uri) else {
            return Ok(Value::Null);
        };
        Ok(self
            .backend
            .handle_document_highlight(&uri, &content, position)
            .map_or(Value::Null, to_value))
    }

    fn rename(&self, params: Value) -> Result<Value, (i64, String)> {
        let params: RenameParams = serde_json::from_value(params).map_err(bad_params)?;
        let position = params.text_document_position.position;
        let uri = params.text_document_position.text_document.uri;
        let Some((uri, content)) = self.buffer(&uri) else {
            return Ok(Value::Null);
        };
        match self
            .backend
            .handle_rename(&uri, &content, position, &params.new_name)
        {
            Ok(edit) => Ok(edit.map_or(Value::Null, to_value)),
            // The move's destination is taken; the reason is the whole
            // point of the response.
            Err(message) => Err((INVALID_REQUEST, message)),
        }
    }

    fn signature_help(&self, params: Value) -> Result<Value, (i64, String)> {
        let params: SignatureHelpParams = serde_json::from_value(params).map_err(bad_params)?;
        let position = params.text_document_position_params.position;
        let uri = params.text_document_position_params.text_document.uri;
        let Some((uri, content)) = self.buffer(&uri) else {
            return Ok(Value::Null);
        };
        Ok(self
            .backend
            .handle_signature_help(&uri, &content, position)
            .map_or(Value::Null, to_value))
    }

    /// The stored buffer for `uri`, as the `(uri, content)` pair the `handle_*`
    /// methods take. `None` when the document was never opened.
    fn buffer(&self, uri: &Url) -> Option<(String, String)> {
        let uri = uri.to_string();
        let content = self.backend.get_file_content(&uri)?;
        Some((uri, content))
    }

    /// The `initialize` result. Advertises exactly what `handle` routes, so a
    /// client never asks for a request that would come back as null.
    fn capabilities(&self) -> Value {
        json!({
            "capabilities": {
                // Full sync: the host sends the whole buffer on every edit.
                "textDocumentSync": 1,
                "completionProvider": {
                    "triggerCharacters": [">", ":", "$", "\\"],
                    "resolveProvider": true
                },
                "hoverProvider": true,
                "definitionProvider": true,
                "documentHighlightProvider": true,
                "renameProvider": true,
                "signatureHelpProvider": { "triggerCharacters": ["(", ","] }
            },
            // The same name and version the native server reports, so a host
            // can tell which build it is talking to.
            "serverInfo": {
                "name": self.backend.name,
                "version": self.backend.version,
            }
        })
    }
}

/// Serialize a handler's response. Every LSP response type serializes
/// infallibly, so the error arm is unreachable in practice.
fn to_value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn bad_params(error: serde_json::Error) -> (i64, String) {
    (INVALID_PARAMS, error.to_string())
}
