//! Shared request plumbing for the LSP handlers in [`crate::server`]:
//! open-file access behind a panic guard, the position/URI handler
//! wrappers, and the whole-file request coalescer.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use tower_lsp_server::gen_lsp_types::Position;
use tower_lsp_server::jsonrpc::Result;

use parking_lot::Mutex;

use crate::Backend;
use crate::server::run_blocking_cancel_safe;

/// The caches every LSP request resolves types under, held for the
/// request's lifetime.
///
/// Entered by [`Backend::with_file_content`], by the entry points that
/// fetch their own content (completion and its resolve, code-action
/// resolve), and by the `handle_*` methods that in-process callers reach
/// without a request (hover, code actions), so no feature answers with a
/// colder cache than the one next to it.  Each guard is re-entrant:
/// entering a scope inside another one keeps the outer caches.  Fields drop in declaration order, so the parse cache,
/// the most recently installed guard, is released first.
pub(crate) struct RequestScope<'a> {
    // Parse the document at most once per request.  The type engine
    // reaches the AST through `with_parsed_program` from many places
    // (variable resolution, closures, property narrowing, …); without the
    // cache each of those re-parses the whole file, so a hover or
    // completion that resolves a few expressions parses it several times
    // over while diagnostics parse it once.
    _parse: crate::parser::ParseCacheGuard<'a>,
    // Hand the workspace's resolved classes to the handler.  A class
    // resolution that is not given a cache builds a throwaway one, so
    // without this every call that reaches `resolve_class_fully` re-runs
    // the whole inheritance merge, virtual-member synthesis, and (on
    // Laravel) builder forwarding for a model that the diagnostic pass
    // resolved long ago.
    _resolved_classes: crate::virtual_members::ResolvedCacheGuard,
    _type_engine: crate::type_engine::call_resolution::TypeEngineCaches,
    // Shared chain prefixes (e.g. `$model->where(...)` in
    // `$model->where(...)->orderBy(...)`) are resolved once and reused.
    _chain: crate::type_engine::resolver::ChainCacheGuard,
    // Classes are looked up as the Composer project holding the file
    // sees them.  Entered first, so it outlives every cache filled
    // under it.
    _project: crate::resolution::project_scope::ProjectScopeGuard,
}

impl Backend {
    /// Enter a [`RequestScope`] for `content`, the text of `uri`, which
    /// the parse cache shares instead of copying.
    pub(crate) fn enter_request_scope(
        &self,
        uri: &str,
        content: Arc<String>,
    ) -> RequestScope<'static> {
        self.request_scope_with(uri, || crate::parser::with_parse_cache_arc(content))
    }

    /// [`enter_request_scope`](Self::enter_request_scope) for a caller
    /// that only borrows the content.
    pub(crate) fn enter_request_scope_borrowed<'a>(
        &self,
        uri: &str,
        content: &'a str,
    ) -> RequestScope<'a> {
        self.request_scope_with(uri, || crate::parser::with_parse_cache(content))
    }

    fn request_scope_with<'a>(
        &self,
        uri: &str,
        parse: impl FnOnce() -> crate::parser::ParseCacheGuard<'a>,
    ) -> RequestScope<'a> {
        let _project = self.enter_project_scope(uri);
        let _chain = crate::type_engine::resolver::with_chain_resolution_cache();
        let _type_engine = crate::type_engine::call_resolution::activate_type_engine_caches();
        let _resolved_classes =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);
        RequestScope {
            _parse: parse(),
            _resolved_classes,
            _type_engine,
            _chain,
            _project,
        }
    }

    /// Fetch the open-file content for `uri`, run `f` inside a panic
    /// guard, and return the result.
    ///
    /// Returns `None` when the file is not open or when `f` panics.
    /// Most LSP handlers follow the pattern "get content, run handler
    /// with panic protection, return result" — this helper captures
    /// that boilerplate in one place.
    pub(crate) fn with_file_content<T>(
        &self,
        handler_name: &str,
        uri: &str,
        position: Option<Position>,
        f: impl FnOnce(&str, Option<Position>) -> T,
    ) -> Option<T> {
        // A request can arrive before the file's first parse has published
        // a symbol map: an editor fires hover the instant it opens a file
        // (tower-lsp may run the request handler before `did_open` finishes
        // `update_ast`), and a raw LSP client may query a file it never
        // opened at all, ahead of background indexing.  Without the map the
        // handler answers null or falls back to poorer resolution, so the
        // same request succeeds or fails with the indexing timing.  Parse
        // now from the content we already fetched.  The parse publishes the
        // map, so a file passes through here once; a file the parser panics
        // on publishes nothing and is retried, which is the same work its
        // next keystroke would do anyway.
        if !self.is_resource_uri(uri) && self.symbol_map_for(uri).is_none() {
            let content = self.get_file_content(uri)?;
            self.update_ast(uri, &content);
        }

        // A template is analysed as the virtual PHP it lowers to, with the
        // position moved into it.
        let (content, pos) = match position {
            Some(position) => {
                let (content, pos) = self.analysable_content_at(uri, position)?;
                (content, Some(pos))
            }
            None => (self.analysable_content(uri)?, None),
        };

        let _scope = self.enter_request_scope(uri, Arc::clone(&content));
        crate::util::catch_panic_unwind_safe(handler_name, uri, pos, || f(&content, pos))
    }

    /// Position-based handler helper. Extracts the URI and position from
    /// the params, fetches file content, runs the closure inside a panic
    /// guard, and flattens the nested `Option`.
    ///
    /// Covers the majority of LSP handlers that take a
    /// `TextDocumentPositionParams` and return `Option<T>`.
    pub(crate) fn handle_with_position<T>(
        &self,
        handler_name: &str,
        uri: &str,
        position: Position,
        f: impl FnOnce(&str, Position) -> Option<T>,
    ) -> Result<Option<T>> {
        Ok(self
            .with_file_content(
                handler_name,
                uri,
                Some(position),
                |content, pos| match pos {
                    Some(pos) => f(content, pos),
                    None => None,
                },
            )
            .flatten())
    }

    /// URI-only handler helper. Like [`handle_with_position`] but for
    /// handlers that only need the document URI (no cursor position).
    pub(crate) fn handle_with_uri<T>(
        &self,
        handler_name: &str,
        uri: &str,
        f: impl FnOnce(&str) -> Option<T>,
    ) -> Result<Option<T>> {
        Ok(self
            .with_file_content(handler_name, uri, None, |content, _| f(content))
            .flatten())
    }

    /// [`with_file_content`](Self::with_file_content) for a feature that
    /// maps symbol-map offsets onto the buffer it answers for.
    ///
    /// `did_change` rebuilds the map on a background task, so a request
    /// arriving between a keystroke and that rebuild sees a map built from
    /// older text, whose offsets land a keystroke's worth away from their
    /// symbols.  Such a request answers `ContentModified` instead of a
    /// result: the client keeps what it shows until the refresh
    /// `did_change` sends once the new map commits, or, for a feature with
    /// no refresh, re-requests on the next cursor move.  A file with no map
    /// (the parser gave up on it, or a resource document) is not declined,
    /// since no re-request would fix that.
    pub(crate) fn with_current_symbol_map<T>(
        &self,
        handler_name: &str,
        uri: &str,
        position: Option<Position>,
        f: impl FnOnce(&str, Option<Position>) -> Option<T>,
    ) -> Result<Option<T>> {
        let outcome = self.with_file_content(handler_name, uri, position, |content, pos| {
            let checked = self.symbol_map_for(uri);
            if checked
                .as_ref()
                .is_some_and(|map| !map.matches_source(content))
            {
                return None;
            }
            let result = f(content, pos);
            // A map committed while `f` ran may have handed it offsets into
            // text newer than `content`.  Holding `checked` keeps its
            // allocation alive, so an unchanged pointer means `f` read the
            // map that was checked.
            let unchanged = match (&checked, &self.symbol_map_for(uri)) {
                (Some(before), Some(after)) => Arc::ptr_eq(before, after),
                (before, after) => before.is_none() && after.is_none(),
            };
            unchanged.then_some(result)
        });
        match outcome {
            Some(Some(result)) => Ok(result),
            Some(None) => Err(content_modified(handler_name)),
            // Not readable, or the handler panicked.
            None => Ok(None),
        }
    }

    /// Run an expensive whole-file request (`kind`) for `uri` with coalescing.
    ///
    /// The `compute` closure runs on the blocking pool. At most one
    /// computation per `(kind, uri)` runs at a time; a request that is no
    /// longer the most recent of its kind when it acquires the slot returns
    /// the previous result instead of recomputing. This stops a keystroke
    /// burst from piling up dozens of un-cancellable full-file scans that
    /// would otherwise saturate every core and stall completion and hover.
    ///
    /// See [`WholeFileCoalesce`] for the rationale.
    pub(crate) async fn coalesced_whole_file<T, F>(
        &self,
        kind: &'static str,
        uri: &str,
        compute: F,
    ) -> Result<Option<T>>
    where
        T: Clone + Send + Sync + 'static,
        F: FnOnce() -> Result<Option<T>> + Send + 'static,
    {
        let key = WholeFileCoalesce::key(kind, uri);
        let coalesce = &self.whole_file_coalesce;
        let seq = coalesce.stamp(&key);

        let lock = coalesce.key_lock(&key);
        let _guard = lock.lock().await;

        // A newer request of the same kind for this file arrived while we
        // waited: it will produce the fresh result, so skip the scan and hand
        // back the previous result. The editor superseded (and likely already
        // cancelled) this request, so it discards whatever we return — but the
        // cached value avoids any chance of a momentary empty result.
        if !coalesce.is_latest(&key, seq) {
            return Ok(coalesce
                .last_result(&key)
                .and_then(|any| any.downcast_ref::<T>().cloned()));
        }

        let result = run_blocking_cancel_safe(kind, compute)
            .await
            .unwrap_or(Ok(None))?;
        if let Some(value) = &result {
            coalesce.store_result(
                &key,
                Arc::new(value.clone()) as Arc<dyn std::any::Any + Send + Sync>,
            );
        }
        Ok(result)
    }
}

/// The answer to a request whose result would describe a different
/// revision of the document than the one the client holds.
fn content_modified(handler_name: &str) -> tower_lsp_server::jsonrpc::Error {
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::ContentModified,
        message: format!("{handler_name} is not current for this document version").into(),
        data: None,
    }
}

/// Request-coalescing state for expensive whole-file requests (semantic
/// tokens, code lens, document symbols, folding, links).
///
/// Editors re-issue these on every keystroke and cancel the superseded ones,
/// but a `spawn_blocking` computation cannot be aborted once it starts, so a
/// fast typist piles up many full-file scans (hundreds of ms each) that all
/// run to completion and saturate every CPU core, starving the cheap
/// interactive requests (completion, hover) until the user gives up waiting.
///
/// This coalesces by `(kind, uri)`: a global sequence stamps each request,
/// a per-key async lock serialises computation so at most one runs per kind
/// per file, and any request that finds itself no longer the latest when it
/// acquires the lock short-circuits to the previous result instead of redoing
/// the scan. A burst of N requests therefore performs at most two scans (the
/// one already running plus the newest) rather than N.
#[derive(Default)]
pub(crate) struct WholeFileCoalesce {
    /// Monotonic request counter shared across all keys.
    seq: AtomicU64,
    /// Latest request sequence seen per `"{kind}\0{uri}"` key.
    latest: Mutex<HashMap<String, u64>>,
    /// Per-key serialisation lock (async, held across the blocking compute).
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Last successfully computed result per key, returned to superseded
    /// requests so the editor never briefly sees an empty result.
    last: Mutex<HashMap<String, Arc<dyn std::any::Any + Send + Sync>>>,
}

impl WholeFileCoalesce {
    /// Get (or create) the per-key async serialisation lock. Held across the
    /// blocking compute so at most one computation per key runs at a time.
    pub(crate) fn key_lock(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock();
        Arc::clone(
            locks
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    /// Stamp a request with the next sequence number and record it as the
    /// latest for `key`. Returns the stamped sequence.
    pub(crate) fn stamp(&self, key: &str) -> u64 {
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.latest.lock().insert(key.to_string(), seq);
        seq
    }

    /// Whether `seq` is still the latest request stamped for `key`.
    pub(crate) fn is_latest(&self, key: &str, seq: u64) -> bool {
        self.latest.lock().get(key).copied() == Some(seq)
    }

    /// Read the cached last result for `key`, if any.
    pub(crate) fn last_result(&self, key: &str) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        self.last.lock().get(key).cloned()
    }

    /// Store the last computed result for `key`.
    pub(crate) fn store_result(&self, key: &str, value: Arc<dyn std::any::Any + Send + Sync>) {
        self.last.lock().insert(key.to_string(), value);
    }

    /// Drop everything kept for `uri` (on `did_close`), so the maps do not
    /// grow across an editing session.
    pub(crate) fn forget_uri(&self, uri: &str) {
        let suffix = Self::key("", uri);
        self.latest.lock().retain(|k, _| !k.ends_with(&suffix));
        self.locks.lock().retain(|k, _| !k.ends_with(&suffix));
        self.last.lock().retain(|k, _| !k.ends_with(&suffix));
    }

    /// The key a `kind` request for `uri` is coalesced under.
    fn key(kind: &str, uri: &str) -> String {
        format!("{kind}\u{0}{uri}")
    }
}

#[cfg(test)]
mod request_scope_tests {
    use crate::Backend;

    /// Every handler resolves classes against the workspace's own store.
    ///
    /// A resolution that is handed no store builds a throwaway one, so a
    /// handler that runs without this re-merges the inheritance, virtual
    /// members, and framework synthesis of every class it touches, however
    /// many times the project has already resolved them.
    #[test]
    fn a_request_resolves_against_the_workspaces_class_store() {
        let backend = Backend::new_test();
        let uri = "file:///Widget.php";
        let source = "<?php\nclass Widget {}\n";
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(source.to_string()));
        backend.update_ast(uri, source);

        let active = backend
            .handle_with_uri("test", uri, |_| {
                Some(crate::virtual_members::active_resolved_class_cache().map(std::ptr::from_ref))
            })
            .expect("the handler runs")
            .expect("the file is readable");

        assert_eq!(
            active,
            Some(std::ptr::from_ref(&backend.resolved_class_cache)),
            "the handler must see this backend's store, not a throwaway"
        );
    }
}

#[cfg(test)]
mod current_symbol_map_tests {
    use std::sync::Arc;

    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::*;

    use crate::Backend;

    const URI: &str = "file:///test/lagging_map.php";
    const SOURCE: &str = "<?php\nclass Widget {\n    public function run(): void {}\n}\n\
                          function useWidget(Widget $widget): void { $widget->run(); }\n";

    /// A backend whose buffer is one keystroke ahead of the symbol map
    /// describing it, the state a background `did_change` parse leaves
    /// behind until it commits.
    fn backend_with_lagging_map() -> Backend {
        let backend = Backend::new_test();
        backend
            .open_files
            .write()
            .insert(URI.to_string(), Arc::new(SOURCE.to_string()));
        backend.update_ast(URI, SOURCE);
        backend.mark_workspace_indexed();
        let edited = SOURCE.replace("class Widget", "\nclass Widget");
        backend
            .open_files
            .write()
            .insert(URI.to_string(), Arc::new(edited));
        backend
    }

    fn assert_content_modified<T: std::fmt::Debug>(
        feature: &str,
        answer: tower_lsp_server::jsonrpc::Result<T>,
    ) {
        match answer {
            Err(error) => assert_eq!(
                error.code,
                tower_lsp_server::jsonrpc::ErrorCode::ContentModified,
                "{feature}: a decline must be ContentModified, not any other error"
            ),
            Ok(answer) => panic!("{feature} answered {answer:?} from a lagging map"),
        }
    }

    fn document() -> TextDocumentIdentifier {
        TextDocumentIdentifier {
            uri: Uri::parse(URI).unwrap(),
        }
    }

    #[tokio::test]
    async fn semantic_tokens_decline_while_the_map_lags_the_buffer() {
        let backend = backend_with_lagging_map();
        let answer = backend
            .semantic_tokens_full(SemanticTokensParams {
                text_document: document(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await;
        assert_content_modified("semantic tokens", answer);
    }

    #[tokio::test]
    async fn code_lens_declines_while_the_map_lags_the_buffer() {
        let backend = backend_with_lagging_map();
        let answer = backend
            .code_lens(CodeLensParams {
                text_document: document(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await;
        assert_content_modified("code lens", answer);
    }

    #[tokio::test]
    async fn document_highlight_declines_while_the_map_lags_the_buffer() {
        let backend = backend_with_lagging_map();
        let answer = backend
            .document_highlight(DocumentHighlightParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: document(),
                    position: Position::new(2, 9),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await;
        assert_content_modified("document highlight", answer);
    }

    /// Once the parse commits, the same requests answer normally.
    #[tokio::test]
    async fn a_committed_map_answers_again() {
        let backend = backend_with_lagging_map();
        let edited = backend.get_file_content(URI).unwrap();
        backend.update_ast(URI, &edited);

        let tokens = backend
            .semantic_tokens_full(SemanticTokensParams {
                text_document: document(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await;
        assert!(
            matches!(tokens, Ok(Some(ref tokens)) if !tokens.data.is_empty()),
            "{tokens:?}"
        );

        // `Widget` in `useWidget(Widget $widget)`, one line lower now.
        let highlights = backend
            .document_highlight(DocumentHighlightParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: document(),
                    position: Position::new(5, 21),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await;
        assert!(
            matches!(highlights, Ok(Some(ref h)) if h.len() == 2),
            "{highlights:?}"
        );
    }
}

#[cfg(test)]
mod coalesce_tests {
    use crate::Backend;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A burst of concurrent whole-file requests for the same `(kind, uri)`
    /// must coalesce: only a small number actually compute, and the rest
    /// short-circuit. This is the mechanism that stops a keystroke burst from
    /// piling up un-cancellable full-file scans and starving completion.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn coalesced_whole_file_collapses_a_burst() {
        const N: usize = 20;

        let backend = Arc::new(Backend::new_test());
        let computes = Arc::new(AtomicUsize::new(0));

        // Fire N concurrent requests for the same kind+uri. Each "computation"
        // is deliberately slow so the whole burst arrives while the first one
        // is still running — exactly the editor's keystroke-burst pattern.
        let mut handles = Vec::new();
        for _ in 0..N {
            let b = Arc::clone(&backend);
            let c = Arc::clone(&computes);
            handles.push(tokio::spawn(async move {
                b.coalesced_whole_file("test_kind", "file:///burst.php", move || {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(100));
                    Ok(Some(n))
                })
                .await
            }));
        }

        let mut results = Vec::new();
        for h in handles {
            results.push(h.await.unwrap().unwrap());
        }

        let computed = computes.load(Ordering::SeqCst);
        assert!(computed >= 1, "at least one request must actually compute");
        assert!(
            computed < N,
            "burst should coalesce: {computed} of {N} requests computed (no coalescing)"
        );
        // The latest request always gets a freshly computed value; superseded
        // ones get either the cached value or None, but never block forever.
        assert!(
            results.iter().any(|r| r.is_some()),
            "at least one request must return a result"
        );
    }

    /// Requests for *different* files are not serialised against each other:
    /// distinct `(kind, uri)` keys each compute independently.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn coalesced_whole_file_is_per_uri() {
        let backend = Arc::new(Backend::new_test());
        let computes = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for i in 0..4 {
            let b = Arc::clone(&backend);
            let c = Arc::clone(&computes);
            let uri = format!("file:///file{i}.php");
            handles.push(tokio::spawn(async move {
                b.coalesced_whole_file("test_kind", &uri, move || {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(i))
                })
                .await
            }));
        }
        for h in handles {
            let _ = h.await.unwrap().unwrap();
        }

        assert_eq!(
            computes.load(Ordering::SeqCst),
            4,
            "each distinct file must compute independently"
        );
    }
}
