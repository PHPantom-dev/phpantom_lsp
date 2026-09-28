//! Eager cache population for the LSP server.
//!
//! Two passes run once the workspace is fully indexed, both so that the
//! first interactive request reads an answer instead of computing one:
//!
//! - Every known class is resolved in dependency-first order, mirroring
//!   the CLI analyse pipeline, so completion, hover, diagnostics, and
//!   go-to-definition read pre-resolved metadata from the cache instead
//!   of recursing into class resolution.
//! - Every user file's member accesses have their receiver resolved, so
//!   the first Find References or reference CodeLens of the session
//!   answers from the recorded receivers instead of walking each
//!   candidate file with the type engine.
//!
//! Incremental re-population on file change is handled separately:
//! `parser/ast_update.rs` re-resolves only the classes an edit evicted,
//! and the receiver entries an edit can have changed are dropped by the
//! dependency-keyed retention in `reference_index.rs`.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::Backend;

/// How long the receiver warm-up may run before it stops where it is.
///
/// Every entry it writes stands on its own, so stopping early leaves a
/// smaller warm layer rather than a broken one: the files it did not
/// reach are walked by the first search that needs them, exactly as they
/// were before the pass existed.  That is what makes a budget the right
/// shape here — the whole value of the pass is being finished before the
/// user's first search, so a workspace big enough to blow through this is
/// one where finishing was never going to happen in time anyway.
const RECEIVER_WARMUP_BUDGET: Duration = Duration::from_secs(30);

impl Backend {
    /// Wait until `initialized` has finished, returning `false` when
    /// the server shuts down first.
    ///
    /// Startup tasks that populate resolution caches must wait for
    /// this: `initialized` clears those caches after the startup scan,
    /// which would discard any population that ran earlier.
    pub(crate) async fn wait_for_init_complete(&self) -> bool {
        while !self.init_complete.load(Ordering::Acquire) {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        true
    }

    /// Resolve every class in `uri_classes_index` in topological
    /// (dependency-first) order, populating `resolved_class_cache`.
    ///
    /// Skips classes that are already cached, so calling this again
    /// (e.g. from the workspace diagnostics pass after the full-index
    /// task already populated) is cheap.
    pub(crate) async fn eager_populate_resolved_classes(&self) {
        let backend = self.clone_for_blocking();
        crate::server::run_blocking_cancel_safe("eager_populate_resolved_classes", move || {
            let sorted_fqns = {
                let uri_classes_index = backend.symbols.uri_classes_index.read();
                crate::toposort::toposort_from_uri_classes_index(&uri_classes_index)
            };
            // `populate_from_sorted` fans the list out over its own
            // large-stack workers, so this needs no wrapper thread of
            // its own.
            let class_loader = |name: &str| backend.find_or_load_class(name);
            crate::virtual_members::populate_from_sorted(
                &sorted_fqns,
                &backend.resolved_class_cache,
                &class_loader,
            );
        })
        .await;
    }

    /// Resolve the receiver of every member access in every user file,
    /// recording what each one resolved to.
    ///
    /// A member search selects its candidate files by member name alone, so
    /// a name as common as `save` or `handle` selects every file that
    /// accesses *any* class's member of that name.  Telling those apart
    /// means running the type engine over each of them, and it is the walk
    /// that costs: resolving one receiver means forward-walking the body it
    /// sits in from its first statement.  A file that has been walked once
    /// answers for every access it holds, so doing that here — once per
    /// file, in the background, while nobody is waiting — is what keeps the
    /// first search of a session from paying for it in the foreground.
    ///
    /// Entries survive edits elsewhere in the workspace: each one records
    /// the classes and functions its resolution consulted, and only the
    /// entries an edit can have changed are dropped.
    pub(crate) async fn warm_member_reference_layer(&self) {
        if self.skip_reference_index {
            return;
        }
        let backend = self.clone_for_blocking();
        crate::server::run_blocking_cancel_safe("warm_member_reference_layer", move || {
            backend.warm_member_reference_layer_blocking();
        })
        .await;
    }

    fn warm_member_reference_layer_blocking(&self) {
        // A file with no member access has no receiver to resolve, and
        // claiming it would cost a worker a symbol-map lookup to find that
        // out.
        let uris: Vec<String> = self
            .user_file_symbol_maps()
            .into_iter()
            .filter(|(_, symbol_map)| !symbol_map.member_access_indices.is_empty())
            .map(|(uri, _)| uri)
            .collect();
        if uris.is_empty() {
            return;
        }

        let started = Instant::now();
        // A quarter of the available cores, so interactive requests stay
        // responsive while the pass runs and the pass keeps its memory
        // down.  Each worker holds a parse cache and a whole file's scope
        // snapshots at once, and on a 1,400-file Laravel application a
        // quarter of 32 cores left resident memory where it was while half
        // of them raised it by a tenth, for 0.85 s of background wall
        // against 0.6 s.
        let workers = std::thread::available_parallelism()
            .map(|n| (n.get() / 4).max(2))
            .unwrap_or(2);
        let walked = crate::parallel::map_indexed_with_threads(
            "member-warmup",
            uris.len(),
            Some(workers),
            |_, index| {
                if self.shutdown_flag.load(Ordering::Acquire)
                    || started.elapsed() > RECEIVER_WARMUP_BUDGET
                {
                    return None;
                }
                let uri = &uris[index];
                crate::util::catch_panic_unwind_safe("member_warmup", uri, None, || {
                    self.warm_member_receivers(uri)
                })
                .unwrap_or(false)
                .then_some(())
            },
        )
        .len();

        tracing::info!(
            "warm_member_reference_layer: walked {walked} of {} files in {:?}",
            uris.len(),
            started.elapsed()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use crate::Backend;

    fn parse_file(backend: &Backend, uri: &str, text: &str) {
        backend
            .open_files
            .write()
            .insert(uri.to_string(), std::sync::Arc::new(text.to_string()));
        backend.update_ast(uri, text);
        backend.workspace_indexed.store(true, Ordering::Release);
    }

    /// The pass reaches every user file, not just the ones a search has
    /// already had a reason to open.
    #[test]
    fn the_warm_up_records_receivers_for_every_user_file() {
        const SERVICE_URI: &str = "file:///Service.php";
        const CONSUMER_URI: &str = "file:///Consumer.php";
        const EMPTY_URI: &str = "file:///Empty.php";

        let backend = Backend::new_test();
        parse_file(
            &backend,
            SERVICE_URI,
            "<?php\nclass Service {\n    public function save(): void {}\n}\n",
        );
        parse_file(
            &backend,
            CONSUMER_URI,
            "<?php\nfunction run(Service $service): void {\n    $service->save();\n}\n",
        );
        parse_file(&backend, EMPTY_URI, "<?php\nclass Empty_ {}\n");

        backend.warm_member_reference_layer_blocking();

        let symbol_map = backend
            .symbol_maps
            .read()
            .get(CONSUMER_URI)
            .cloned()
            .expect("the consumer was parsed");
        let entry = backend
            .resolved_member_file(CONSUMER_URI, &symbol_map)
            .expect("the pass walked the consumer");
        assert!(entry.covers([crate::atom::atom("save")]));

        let empty_map = backend
            .symbol_maps
            .read()
            .get(EMPTY_URI)
            .cloned()
            .expect("the empty file was parsed");
        assert!(
            backend
                .resolved_member_file(EMPTY_URI, &empty_map)
                .is_none(),
            "a file with no member access is not claimed by a worker at all"
        );
    }
}
