//! Diagnostics — collect and deliver LSP diagnostics for PHP files.
//!
//! This module collects diagnostics from multiple providers and delivers
//! them to the editor.
//!
//! ## Diagnostic code naming convention
//!
//! Every diagnostic has a `code` string that identifies the rule. When adding
//! a new diagnostic, follow these rules:
//!
//! 1. **All `snake_case`, no dots or other separators.**
//! 2. **Codes read as noun phrases describing the problem**, not bare
//!    categories. Prefer `argument_count_mismatch` over `argument_count`.
//! 3. **`unknown_*`** — symbol could not be resolved (class, function,
//!    member, variable).
//! 4. **`unused_*`** — symbol is defined/imported but never referenced.
//! 5. **`type_mismatch_*`** — a value's type doesn't satisfy a constraint
//!    (`type_mismatch_argument`, `type_mismatch_return`,
//!    `type_mismatch_property`).
//! 6. **`missing_*`** — a required declaration is absent (e.g.
//!    `missing_implementation` for unimplemented interface methods).
//! 7. **`invalid_*`** — a structural/syntactic violation (e.g.
//!    `invalid_class_kind`).
//! 8. **`deprecated_usage`** — usage of a deprecated symbol.
//! 9. **`syntax_error`** — parser-level errors.
//! 10. **`unresolved_*`** — the analyser couldn't determine the type of an
//!     expression (opt-in coverage hints).
//!
//! Two native delivery models are supported:
//!
//! - **Pull model** (`textDocument/diagnostic`, LSP 3.17) — the editor
//!   requests diagnostics when it needs them.  Only visible files are
//!   diagnosed.  Cross-file invalidation uses `workspace/diagnostic/refresh`.
//!   This is the preferred model when the client supports it.
//!
//! - **Push model** (`textDocument/publishDiagnostics`) — the server
//!   pushes diagnostics after every edit.  Used as a fallback for clients
//!   that do not advertise pull-diagnostic support.
//!
//! Native diagnostics are intentionally sent through exactly one of these
//! channels per client.  When pull is available, PHPantom uses pull only;
//! push is reserved for clients that do not support pull.  Mixing both for
//! the same native diagnostics makes client-side merge behavior ambiguous and
//! can surface duplicates or flicker.
//!
//! Providers are grouped into three phases so that cheap results appear
//! immediately and expensive external tools never block native feedback:
//!
//! ## Phase 1 — fast (no type resolution)
//!
//! - **Syntax error diagnostics** — surface parse errors from the Mago
//!   parser as Error-severity diagnostics.  The most fundamental
//!   diagnostic: without it, a user with a typo gets no feedback until
//!   they try to run the code.
//! - **Unused `use` dimming** — dim `use` declarations that are not
//!   referenced anywhere in the file with `DiagnosticTag::Unnecessary`.
//! - **Unused variable diagnostics** — dim variables that are assigned
//!   or bound as a parameter but never read in the same scope.
//! - **Namespace mismatch diagnostics** — report a single-class file
//!   whose declared namespace disagrees with the PSR-4 mapping derived
//!   from its path.
//! - **Class name mismatch diagnostics** — report a single-class file
//!   whose class name disagrees with the name PSR-4 expects for its path.
//! - **Unreachable code diagnostics** — dim the statements that follow one
//!   which always leaves the block (`return`, `throw`, `exit`, `die`,
//!   `continue`, `break`, or an `if` whose every branch does).  Reads the
//!   shape of the statement list only, so it needs no type resolution.
//!
//! ## Phase 2 — slow (require type resolution)
//!
//! - **Unknown class diagnostics** — report `ClassReference` spans that
//!   cannot be resolved through any resolution phase (use-map, local
//!   classes, same-namespace, fqn_uri_index, PSR-4, stubs).
//! - **Unknown member diagnostics** — report `MemberAccess` spans where
//!   the member does not exist on the resolved class after full
//!   resolution (inheritance + virtual member providers).  Suppressed
//!   when the class has `__call` / `__callStatic` / `__get` magic methods.
//! - **Unknown function diagnostics** — report function calls that
//!   cannot be resolved to any known function definition.
//! - **Undefined variable diagnostics** — report variable reads that
//!   have no prior definition (assignment, parameter, foreach binding,
//!   catch variable, `global`, `static`, `use()` clause, or `list()`
//!   destructuring) in the same scope.  Uses a conservative Phase 1
//!   approach: any assignment anywhere in the function counts as a
//!   definition.  Suppressed for superglobals, `isset()` / `empty()`
//!   guards, `compact()` references, `extract()` calls, variable
//!   variables (`$$`), `@` error suppression, and `@var` annotations.
//! - **Unresolved member access diagnostics** (opt-in) — report
//!   `MemberAccess` spans where the **subject type** cannot be resolved
//!   at all.  Off by default; enable via `[diagnostics]
//!   unresolved-member-access = true` in `.phpantom.toml`.  Uses
//!   `Severity::HINT` to surface type-coverage gaps without drowning
//!   the editor in warnings.
//! - **Argument count diagnostics** — report calls where the number of
//!   arguments does not match the function/method signature.
//! - **Implementation error diagnostics** — report concrete classes that
//!   fail to implement all required methods from their interfaces or
//!   abstract parents.  Reuses the same missing-method detection as the
//!   "Implement missing methods" code action.
//! - **Incompatible trait property diagnostics** — report a class or
//!   trait that picks up the same property from two traits (or a trait
//!   and its own body) with declarations PHP rejects as incompatible.
//! - **`@deprecated` usage diagnostics** — report references to symbols
//!   marked `@deprecated` with `DiagnosticTag::Deprecated` (renders as
//!   strikethrough in most editors).  Requires resolving the reference
//!   to its declaration, so this runs here rather than in Phase 1.
//! - **Class case mismatch diagnostics** — report a class reference
//!   whose spelling differs only in case from its PSR-4 declaration,
//!   which loads fine on case-insensitive filesystems but fatals on
//!   Linux.
//! - **Type mismatch diagnostics** — report argument, return, and
//!   property-assignment values whose type does not satisfy the
//!   declared/inferred type.
//! - **Member visibility diagnostics** — report a `private` or
//!   `protected` property, method, or class constant reached from a
//!   scope PHP does not let see it.  Emitted from the unknown-member
//!   walk, which has already resolved the subject.  Suppressed when the
//!   class declares a magic handler that would answer for the member.
//! - **Readonly write diagnostics** — report writes to a `readonly`
//!   property from outside the class that declares it, and second
//!   writes from inside it once the constructor has initialized the
//!   property.
//! - **Invalid class kind diagnostics** — report a class-like name used
//!   in a syntactic position (`new`, `implements`, `instanceof`, …) that
//!   its kind (class/interface/trait/enum) cannot satisfy.
//! - **Laravel string key / command parameter diagnostics** (Laravel
//!   projects only) — report route/config/view/translation/command
//!   names and morph aliases that don't resolve to a known declaration,
//!   and `$this->argument()` / `$this->option()` calls that aren't in
//!   the enclosing command's `$signature`.
//!
//! ## Phase 3 — heavy (external process, dedicated workers)
//!
//! - **PHPStan proxy diagnostics** — run PHPStan in editor mode
//!   (`--tmp-file` / `--instead-of`) and surface its errors as LSP
//!   diagnostics.  Auto-detected via `vendor/bin/phpstan` or `$PATH`;
//!   configurable in `.phpantom.toml` under `[phpstan]`.
//!
//!   PHPStan runs in a **dedicated worker task**, separate from the
//!   main diagnostic worker, because it is extremely slow and
//!   resource-intensive.  At most one PHPStan process runs at a time.
//!   If edits arrive while PHPStan is running, the pending URI is
//!   updated and the worker picks it up after the current run finishes.
//!   Native diagnostics (phases 1 and 2) are never blocked.
//!
//! - **PHPCS proxy diagnostics** — run PHP_CodeSniffer via
//!   `phpcs --report=json` and surface coding standard violations as
//!   LSP diagnostics.  Auto-detected via `vendor/bin/phpcs` or `$PATH`,
//!   independent of what `composer.json` declares; configurable under
//!   `[phpcs]`.
//!
//!   PHPCS runs in its own **dedicated worker task**, following the
//!   same pattern as the PHPStan worker.  At most one PHPCS process
//!   runs at a time, with the same debounce and pending-URI slot
//!   design.
//!
//! - **PHPMD proxy diagnostics** — run PHP Mess Detector 3 via
//!   `phpmd analyze --format=json -` and surface its findings as LSP
//!   diagnostics.  Auto-detected via `vendor/bin/phpmd` or `$PATH` when
//!   the project has a PHPMD config file; configurable under `[phpmd]`.
//!   Runs in its own **dedicated worker task**, like PHPCS.
//!
//! - **Mago lint proxy diagnostics** — run `mago lint --reporting-format
//!   json --stdin-input` and surface AST-level lint issues (style,
//!   naming, code smells) as LSP diagnostics.  Auto-detected when
//!   `mago.toml` exists at the workspace root and `vendor/bin/mago` or
//!   `mago` on `$PATH` is available; configurable under `[mago]`.
//!
//!   Mago lint runs in its own **dedicated worker task**, following the
//!   same pattern as the PHPCS worker.  Source: `"mago-lint"`.
//!
//! - **Mago analyze proxy diagnostics** — run `mago analyze
//!   --reporting-format json --stdin-input` and surface type-aware
//!   analysis issues (type mismatches, unreachable code, unused
//!   definitions) as LSP diagnostics.  Same auto-detection as Mago lint.
//!
//!   Mago analyze runs in its own **dedicated worker task**, following
//!   the same pattern as the PHPStan worker.  Source: `"mago-analyze"`.
//!
//! ## Publishing strategy
//!
//! Each diagnostic source has its own per-URI cache:
//!
//! | Cache                    | Source             |
//! | ------------------------ | ------------------ |
//! | `diag_last_fast`           | Phase 1 (fast)     |
//! | `diag_last_slow`           | type resolution    |
//! | `phpstan_tool.last_diags`  | PHPStan            |
//! | `phpcs_tool.last_diags`    | PHPCS              |
//! | `phpmd_tool.last_diags`    | PHPMD              |
//! | `mago_lint_tool.last_diags`| Mago lint          |
//! | `mago_analyze_tool.last_diags` | Mago analyze  |
//!
//! When any source finishes, [`Backend::assemble_and_push`] reads all
//! per-source caches for the URI, merges them into a single set,
//! deduplicates, and filters suppressions.
//!
//! **Push mode:** The merged set is published via
//! `textDocument/publishDiagnostics`.  As each source finishes, its
//! cache is updated and the full assembled set is pushed.  The user
//! sees results incrementally: fast diagnostics first, then slow,
//! then PHPStan/PHPCS/Mago as each completes.
//!
//! **Pull mode:** Nothing is pushed via `publishDiagnostics`.  The
//! merged set is cached in `diag_last_full` with a bumped `resultId`
//! and the editor is asked to re-pull via `workspace/diagnostic/refresh`.
//! A source run that reproduces the cached set exactly leaves both the
//! cache and the `resultId` alone and sends no refresh: LSP has no
//! per-document refresh, so each one costs a workspace-wide re-pull and
//! is worth sending only for a real change.
//! The pull handler (`textDocument/diagnostic`) returns this cached set.
//! If the cache is missing (e.g. the file was just opened), the pull
//! handler schedules a background computation and returns whatever is
//! cached right now — usually empty — instead of computing inline: a
//! synchronous compute on the request path can wedge the transport
//! under a typing burst (see [`trigger_diagnostics_for_pull`]). The
//! worker's completion then bumps the `resultId` and requests a
//! `workspace/diagnostic/refresh`, so the editor re-pulls with the real
//! results a moment later.  A pull-capable client already has a
//! canonical native diagnostic stream, so pushing the same native
//! diagnostics as well would create two competing streams that clients
//! may merge differently.
//!
//! External tool workers (PHPStan, PHPCS, Mago) use their own
//! debounce timers in both modes because they are expensive.
//!
//! ## Edits inside function bodies
//!
//! The slow phase keeps what its last pass found: when the edit since
//! then only touched some function and method bodies, it diagnoses
//! those again and moves the rest of the file's slow diagnostics to
//! their new positions.  See [`incremental`] for when that is sound.
//!
//! ## Files that are not open
//!
//! Everything above covers the files the user has open.  Diagnostics
//! for the rest of the workspace are computed by a separate background
//! pass described in [`workspace`].

mod argument_count;
mod blade_call_site;
mod blade_component_tags;
mod blade_directives;
mod blade_imbalance;
mod blade_sections;
mod blade_signature;
pub(crate) mod class_case_mismatch;
mod class_init_facts;
pub(crate) mod class_name_mismatch;
mod collect;
pub(crate) mod cross_file;
mod delivery;
mod deprecated;
mod destructuring_offsets;
mod docblock_native_mismatch;
mod enum_errors;
pub(crate) mod existence_guards;
mod external;
pub(crate) mod helpers;
pub(crate) mod ignore_rules;
mod implementation_errors;
mod incompatible_override;
pub(crate) mod incremental;
mod invalid_class_kind;
mod laravel_string_keys;
mod match_type_errors;
pub(crate) mod member_lookup;
pub(crate) mod member_visibility;
pub(crate) mod namespace_mismatch;
mod property_type_errors;
mod pull;
mod readonly_writes;
mod return_type_errors;
mod stale;
pub(crate) mod state;
mod subject_cache;
pub(crate) mod suppression;
mod syntax_errors;
mod trait_property_conflicts;
mod type_errors;
pub(crate) mod undefined_variables;
pub(crate) mod unknown_classes;
pub(crate) mod unknown_functions;
pub(crate) mod unknown_members;
pub(crate) mod unreachable_code;
pub(crate) mod unresolved_member_access;
mod unused_imports;
pub(crate) mod unused_variables;
pub(crate) mod use_statements;
pub(crate) mod workspace;

use tower_lsp::lsp_types::*;

use crate::Backend;

// ── Helpers ─────────────────────────────────────────────────────────────────

impl Backend {
    /// Convert a byte range from the preprocessed (virtual) content back to
    /// an LSP range in the original source file.
    ///
    /// For standard PHP files, this is a straight conversion.  For Blade
    /// files, it converts the bytes to positions in the virtual PHP, then
    /// translates those positions back to original Blade coordinates using
    /// the source map.
    pub(crate) fn offset_range_to_lsp_range(
        &self,
        uri: &str,
        content: &str,
        start_byte: usize,
        end_byte: usize,
    ) -> Option<Range> {
        let blade = self
            .files
            .read()
            .get(uri)
            .and_then(|state| state.blade.clone());
        if let Some(crate::file_state::BladeFileState {
            virtual_php,
            source_map: map,
        }) = &blade
        {
            if start_byte > virtual_php.len() || end_byte > virtual_php.len() {
                return None;
            }

            let mut range =
                crate::text_position::byte_range_to_lsp_range(virtual_php, start_byte, end_byte);

            // A diagnostic originating in the prologue (injected headers)
            // is skipped rather than reported on line 1 of the Blade file.
            range.start = map.try_php_to_blade(range.start)?;
            range.end = map.try_php_to_blade(range.end)?;

            return Some(range);
        }

        // Fallback for standard PHP or if map is missing
        if start_byte > content.len() || end_byte > content.len() {
            return None;
        }

        Some(crate::text_position::byte_range_to_lsp_range(
            content, start_byte, end_byte,
        ))
    }
}

/// Build a diagnostic range from byte offsets, returning `None` if either
/// offset is past the end of `content`.
///
/// This thin wrapper around [`crate::text_position::byte_range_to_lsp_range`] adds
/// a bounds check so that stale byte offsets (e.g. from a previous AST
/// after an edit) are rejected instead of silently clamped to EOF.
///
/// The range stays in the coordinates of `content`: for a Blade template
/// that is the virtual PHP, not the template. A diagnostic range the
/// editor shows must go through [`Backend::offset_range_to_lsp_range`].
pub(crate) fn checked_byte_range_to_lsp_range(
    content: &str,
    start_byte: usize,
    end_byte: usize,
) -> Option<Range> {
    if start_byte > content.len() || end_byte > content.len() {
        return None;
    }
    Some(crate::text_position::byte_range_to_lsp_range(
        content, start_byte, end_byte,
    ))
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_offset_range_to_lsp_range_ignores_prologue() {
        let backend = Backend::new_test();
        let uri = "file:///test.blade.php";
        let content = "Hello World";
        backend.update_ast(uri, content);

        // First 6 lines are prologue (including wrapper function declaration).
        let virtual_php = backend
            .blade_virtual_php_arc(uri)
            .expect("Virtual content should exist");

        // Find start of line 5 (0-indexed).
        let mut offset = 0;
        let mut lines_seen = 0;
        for (i, ch) in virtual_php.char_indices() {
            if lines_seen == 5 {
                offset = i;
                break;
            }
            if ch == '\n' {
                lines_seen += 1;
            }
        }

        // byte range in prologue
        let range = backend.offset_range_to_lsp_range(uri, content, offset, offset + 5);
        assert!(range.is_none(), "Diagnostic in prologue should be ignored");

        // byte range after prologue (line 6+)
        let mut after_offset = 0;
        let mut lines_seen = 0;
        for (i, ch) in virtual_php.char_indices() {
            if lines_seen == 6 {
                after_offset = i;
                break;
            }
            if ch == '\n' {
                lines_seen += 1;
            }
        }

        let range_after =
            backend.offset_range_to_lsp_range(uri, content, after_offset, after_offset + 5);
        assert!(
            range_after.is_some(),
            "Diagnostic after prologue should be kept"
        );
    }

    /// A diagnostic pull must defer to the debounced background worker and
    /// never run the expensive native pass on the request path. Computing
    /// inline holds a tower-lsp concurrency slot for the full multi-second
    /// pass, and editors pull on every keystroke — so a typing burst would
    /// fill every slot and wedge the transport (the regression this guards).
    #[test]
    fn pull_diagnostics_defer_to_worker_and_never_compute_inline() {
        use std::sync::atomic::Ordering;

        let backend = crate::Backend::new_test();
        backend.init_complete.store(true, Ordering::Release);
        backend.client_caps.write().pull_diagnostics = true;

        let uri = "file:///defer.php";
        backend.update_ast(uri, "<?php\nclass A { public function f(): void {} }\n");

        backend.trigger_diagnostics_for_pull(uri);

        assert!(
            backend.diag.pending_uris.lock().contains(uri),
            "pull must schedule the file for the background worker"
        );
        assert!(
            !backend.diag.last_full.lock().contains_key(uri),
            "pull must not compute diagnostics inline on the request path"
        );
    }

    /// Regression test: the `analyse` CLI times each Phase 2 collector
    /// individually via the observer, and must see the whole collector
    /// list.  It used to carry its own copy of the list, which omitted
    /// the Laravel checks, so `analyze` reported no `invalid_laravel_*`
    /// errors on a Laravel project while the LSP reported them.
    #[test]
    fn observed_slow_diagnostics_report_every_collector() {
        let backend = Backend::new_test();
        backend
            .resolved_class_cache
            .write()
            .laravel_mut()
            .set_enabled(true);

        let uri = "file:///app/Http/observed.php";
        let php = "<?php\nclass Observed { public function run(): void { config('app.name'); } }\n";
        backend.update_ast(uri, php);

        let mut names: Vec<&'static str> = Vec::new();
        let mut out = Vec::new();
        backend.collect_slow_diagnostics_observed(
            uri,
            php,
            &mut out,
            Some(&mut |name, _elapsed| {
                names.push(name);
                true
            }),
        );

        for expected in [
            "unknown_class",
            "unknown_member",
            "argument_count_mismatch",
            "invalid_class_kind",
            "invalid_laravel_string_key",
            "invalid_command_parameter",
        ] {
            assert!(
                names.contains(&expected),
                "observer never saw the {expected} collector, only {names:?}"
            );
        }

        // Returning `false` stops the pass — this is how the CLI abandons
        // a file that blows its per-file deadline.
        let mut stopped: Vec<&'static str> = Vec::new();
        let mut out = Vec::new();
        backend.collect_slow_diagnostics_observed(
            uri,
            php,
            &mut out,
            Some(&mut |name, _elapsed| {
                stopped.push(name);
                false
            }),
        );
        assert_eq!(stopped.len(), 1, "observer must be able to stop the pass");
    }

    /// A source run that reproduces the cached diagnostics must not bump
    /// the `resultId` or ask the editor to re-pull.  Every source calls
    /// `assemble_and_push` whenever it finishes and most runs reproduce
    /// the previous result exactly, so bumping unconditionally cost a
    /// workspace-wide re-pull per run (a clean open file burned several
    /// of them during startup alone).
    #[tokio::test]
    async fn assemble_and_push_reports_change_only_on_difference() {
        let backend = Backend::new_test();
        backend.client_caps.write().pull_diagnostics = true;
        let uri = "file:///test.php";

        let diag = |line: u32| Diagnostic {
            range: Range::new(Position::new(line, 0), Position::new(line, 5)),
            severity: Some(DiagnosticSeverity::ERROR),
            message: "boom".to_string(),
            ..Default::default()
        };
        let result_id = || backend.diag.result_ids.lock().get(uri).copied();

        // First assembly: the client has nothing yet, so even an empty
        // set is news.
        assert!(backend.assemble_and_push(uri).await);
        assert_eq!(result_id(), Some(1));

        // Re-running every source over an unchanged file changes
        // nothing, so the client's cached result stays valid.
        assert!(!backend.assemble_and_push(uri).await);
        assert!(!backend.assemble_and_push(uri).await);
        assert_eq!(result_id(), Some(1));

        // A real diagnostic appearing is a change.
        backend
            .diag
            .last_fast
            .lock()
            .insert(uri.to_string(), vec![diag(1)]);
        assert!(backend.assemble_and_push(uri).await);
        assert_eq!(result_id(), Some(2));
        assert!(!backend.assemble_and_push(uri).await);

        // So is a slow source adding to it.
        backend
            .diag
            .last_slow
            .lock()
            .insert(uri.to_string(), vec![diag(7)]);
        assert!(backend.assemble_and_push(uri).await);
        assert_eq!(result_id(), Some(3));
        assert_eq!(
            backend.diag.last_full.lock().get(uri).map(Vec::len),
            Some(2)
        );

        // And so is the file becoming clean again.
        backend.diag.last_fast.lock().remove(uri);
        backend.diag.last_slow.lock().remove(uri);
        assert!(backend.assemble_and_push(uri).await);
        assert_eq!(result_id(), Some(4));
        assert!(!backend.assemble_and_push(uri).await);
        assert_eq!(result_id(), Some(4));
    }

    /// A reopened file must never be assigned a `resultId` the client
    /// could still be holding from before the close. Before the
    /// fix, `result_ids` was a per-file counter that `did_close` reset
    /// to nothing, so a reopened file's ids restarted at 1 and could
    /// climb back through a value the client had cached — a pull with
    /// that stale `previousResultId` would then wrongly answer
    /// `Unchanged` with pre-close diagnostics.
    #[tokio::test]
    async fn reopened_file_never_recycles_a_result_id() {
        let backend = Backend::new_test();
        backend.client_caps.write().pull_diagnostics = true;
        let uri = "file:///reopen.php";
        let result_id = || backend.diag.result_ids.lock().get(uri).copied();

        // The client observes a sequence of ids before closing the file.
        assert!(backend.assemble_and_push(uri).await);
        let seen_before_close = result_id();
        assert!(seen_before_close.is_some());

        backend.diag.last_fast.lock().insert(
            uri.to_string(),
            vec![Diagnostic {
                range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                severity: Some(DiagnosticSeverity::ERROR),
                message: "boom".to_string(),
                ..Default::default()
            }],
        );
        assert!(backend.assemble_and_push(uri).await);
        let last_seen_before_close = result_id();
        assert_ne!(seen_before_close, last_seen_before_close);

        // Close drops the file's cached id and full set entirely.
        backend.clear_diagnostics_for_file(uri).await;
        assert_eq!(result_id(), None);

        // Reopen and recompute enough times that a per-file counter
        // reset to 0 would climb back through every id the client saw
        // before the close.
        backend.diag.last_fast.lock().remove(uri);
        for _ in 0..3 {
            backend.assemble_and_push(uri).await;
            backend.diag.last_fast.lock().insert(
                uri.to_string(),
                vec![Diagnostic {
                    range: Range::new(Position::new(0, 0), Position::new(0, 1)),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: "boom again".to_string(),
                    ..Default::default()
                }],
            );
            backend.assemble_and_push(uri).await;
            assert_ne!(result_id(), seen_before_close);
            assert_ne!(result_id(), last_seen_before_close);
        }
    }

    /// `publish_diagnostics_for_file` must not resurrect a closed file's
    /// caches. Before the fix, both Phase 1 and Phase 2 wrote into
    /// `last_fast`/`last_slow` (and cascaded into `last_full` via
    /// `assemble_and_refresh`) unconditionally, so a close landing
    /// mid-compute left the closed file's caches populated again right
    /// after `clear_diagnostics_for_file` had purged them. A file that
    /// was never in `open_files` exercises the same check.
    #[tokio::test]
    async fn publish_diagnostics_skips_write_when_file_not_open() {
        let backend = Backend::new_test();
        let uri = "file:///not_open.php";
        let content = Arc::new("<?php\nclass Foo {}\n".to_string());

        backend.publish_diagnostics_for_file(uri, &content).await;

        assert!(backend.diag.last_fast.lock().get(uri).is_none());
        assert!(backend.diag.last_slow.lock().get(uri).is_none());
        assert!(backend.diag.last_full.lock().get(uri).is_none());
    }
}
