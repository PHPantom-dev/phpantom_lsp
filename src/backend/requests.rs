//! Shared request plumbing for the LSP handlers in [`crate::server`]:
//! open-file access behind a panic guard, the position/URI handler
//! wrappers, and the whole-file request coalescer.

use std::sync::Arc;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::Position;

use crate::Backend;
use crate::server::run_blocking_cancel_safe;

impl Backend {
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
        if !crate::resource_navigation::is_resource_document(uri)
            && !self.symbol_maps.read().contains_key(uri)
        {
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

        // Activate the chain resolution cache so that shared chain prefixes
        // (e.g. `$model->where(...)` in `$model->where(...)->orderBy(...)`)
        // are resolved once and reused across all LSP handlers, not just
        // diagnostics.  The guard is re-entrant safe: if a diagnostic pass
        // already activated the cache, this is a no-op.
        let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();

        // Activate the type-engine resolvers here rather than per feature.
        // Every LSP handler needs the file content, so this is the one
        // place none of them can bypass: go-to-definition, find-references,
        // signature help, code actions, rename, and inlay hints resolve an
        // expression with the same facilities hover and diagnostics have,
        // instead of a poorer answer for the identical code.
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

        // For the same reason, hand the workspace's resolved classes to the
        // handler.  A class resolution that is not given a cache builds a
        // throwaway one, so without this every call that reaches
        // `resolve_class_fully` without one re-runs the whole inheritance
        // merge, virtual-member synthesis, and (on Laravel) builder
        // forwarding for a model that the diagnostic pass resolved long ago.
        let _resolved_classes_guard =
            crate::virtual_members::with_active_resolved_class_cache(&self.resolved_class_cache);

        // Parse the document at most once per request.  The type engine
        // reaches the AST through `with_parsed_program` from many places
        // (variable resolution, closures, property narrowing, …); without
        // the cache each of those re-parses the whole file, so a hover or
        // completion that resolves a few expressions parses it several
        // times over while diagnostics parse it once.
        let _parse_guard = crate::parser::with_parse_cache_arc(Arc::clone(&content));

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

    /// Run an expensive whole-file request (`kind`) for `uri` with coalescing.
    ///
    /// The `compute` closure runs on the blocking pool. At most one
    /// computation per `(kind, uri)` runs at a time; a request that is no
    /// longer the most recent of its kind when it acquires the slot returns
    /// the previous result instead of recomputing. This stops a keystroke
    /// burst from piling up dozens of un-cancellable full-file scans that
    /// would otherwise saturate every core and stall completion and hover.
    ///
    /// See [`WholeFileCoalesce`](crate::WholeFileCoalesce) for the rationale.
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
        let key = format!("{kind}\u{0}{uri}");
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
