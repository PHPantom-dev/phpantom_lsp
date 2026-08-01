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
//! - **`@deprecated` usage diagnostics** — report references to symbols
//!   marked `@deprecated` with `DiagnosticTag::Deprecated` (renders as
//!   strikethrough in most editors).
//! - **Unused `use` dimming** — dim `use` declarations that are not
//!   referenced anywhere in the file with `DiagnosticTag::Unnecessary`.
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
//!   LSP diagnostics.  Auto-detected when `squizlabs/php_codesniffer`
//!   is in `require-dev`; configurable under `[phpcs]`.
//!
//!   PHPCS runs in its own **dedicated worker task**, following the
//!   same pattern as the PHPStan worker.  At most one PHPCS process
//!   runs at a time, with the same debounce and pending-URI slot
//!   design.
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
//! | `diag_last_fast`           | syntax, unused use |
//! | `diag_last_slow`           | type resolution    |
//! | `phpstan_tool.last_diags`  | PHPStan            |
//! | `phpcs_tool.last_diags`    | PHPCS              |
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
//! The pull handler (`textDocument/diagnostic`) returns this cached set.
//! If the cache is missing (e.g. the file was just opened), the pull
//! handler triggers computation directly instead of returning empty
//! results.  This is not a workaround for a single client; it is the
//! intended server contract.  A pull-capable client already has a canonical
//! native diagnostic stream, so pushing the same native diagnostics as well
//! would create two competing streams that clients may merge differently.
//!
//! External tool workers (PHPStan, PHPCS, Mago) use their own
//! debounce timers in both modes because they are expensive.
//!
//! ## Files that are not open
//!
//! Everything above covers the files the user has open.  Diagnostics
//! for the rest of the workspace are computed by a separate background
//! pass described in [`workspace`].

mod argument_count;
pub(crate) mod class_case_mismatch;
pub(crate) mod class_name_mismatch;
pub(crate) mod cross_file;
mod deprecated;
mod enum_errors;
mod external;
pub(crate) mod helpers;
pub(crate) mod ignore_rules;
mod implementation_errors;
mod incompatible_override;
mod invalid_class_kind;
mod match_type_errors;
pub(crate) mod namespace_mismatch;
mod property_type_errors;
mod pull;
mod return_type_errors;
mod stale;
pub(crate) mod state;
pub(crate) mod suppression;
mod syntax_errors;
mod type_errors;
pub(crate) mod undefined_variables;
pub(crate) mod unknown_classes;
pub(crate) mod unknown_functions;
pub(crate) mod unknown_members;
pub(crate) mod unresolved_member_access;
mod unused_imports;
pub(crate) mod unused_variables;
pub(crate) mod workspace;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp::lsp_types::*;

use crate::Backend;

// ── Shared helpers ──────────────────────────────────────────────────────────

impl Backend {
    /// Returns `true` if the URI should be skipped for diagnostics
    /// (stub files only).  Vendor files are not skipped because
    /// diagnostics only run on files the user has open in the editor,
    /// and users working in monorepos or with `--prefer-source`
    /// packages legitimately edit vendor files.
    fn should_skip_diagnostics(&self, uri_str: &str) -> bool {
        uri_str.starts_with("phpantom-stub://") || uri_str.starts_with("phpantom-stub-fn://")
    }

    /// Collect Phase 1 (fast) diagnostics: syntax errors, unused
    /// imports.  These are cheap — no type resolution.
    pub(crate) fn collect_fast_diagnostics(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        self.collect_syntax_error_diagnostics(uri_str, content, out);
        self.collect_unused_import_diagnostics(uri_str, content, out);
        self.collect_unused_variable_diagnostics(uri_str, content, out);
        self.collect_namespace_mismatch_diagnostics(uri_str, content, out);
        self.collect_class_name_mismatch_diagnostics(uri_str, content, out);
    }

    /// Collect Phase 2 (slow) diagnostics: unknown class/member/function,
    /// argument count, implementation errors, deprecated usage.  These
    /// require type resolution and are expensive.
    pub fn collect_slow_diagnostics(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // Activate the chain resolution cache so that all slow
        // diagnostic collectors share cached intermediate chain
        // prefix results (e.g. `$model->where(...)` resolved once
        // and reused by `$model->where(...)->whereNotNull(...)`).
        // This eliminates O(depth²) re-resolution of shared chain
        // prefixes across unknown_member, argument_count, type_error,
        // and deprecated collectors.
        let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();

        // Activate the callable target cache so that the same method
        // on the same class is resolved at most once across all
        // diagnostic collectors.  For example, `Builder::where` is
        // looked up once and reused for every `$q->where(...)`,
        // `$query->where(...)`, and `Product::query()->where(...)`
        // call site in the file.
        let _callable_guard = crate::type_engine::call_resolution::with_callable_target_cache();
        let _body_infer_guard = self.activate_body_return_inferrer();
        let _auth_user_guard = self.activate_auth_user_resolver();
        let _validation_rules_guard = self.activate_validation_rules_resolver();

        // ── Phase 2: forward-walked diagnostic scope cache ──────
        // Walk every function/method body in the file once with the
        // forward walker, recording scope snapshots at each statement
        // boundary.  All subsequent `resolve_variable_types` calls
        // from diagnostic collectors hit the cache (O(log N) lookup)
        // instead of doing a full backward scan per member-access
        // span.  This eliminates the O(N × depth × file_size) cost
        // that caused multi-minute analysis times on large files.
        let _scope_guard =
            crate::type_engine::variable::forward_walk::with_diagnostic_scope_cache();

        // ── Shared per-file snapshot for the symbol-span collectors ────
        // Built once and reused by the forward-walker warm-up below and
        // by the six collectors that walk `SymbolMap` spans, so a single
        // diagnostic pass reads `symbol_maps`, `uri_classes_index`,
        // `file_imports`, `file_namespaces`, and `resolved_names` once
        // instead of once per collector — and all of them observe the
        // same snapshot even if a concurrent parse updates those maps
        // mid-pass.  `None` when the file has no symbol map yet, which
        // every one of those collectors already treats as a no-op.
        let file_ctx = helpers::FileDiagnosticContext::gather(self, uri_str);

        if let Some(ctx) = &file_ctx {
            let class_loader = self.class_loader(&ctx.file);
            let function_loader_cl = self.function_loader(&ctx.file);
            let constant_loader_cl = self.constant_loader();
            let config_resolver = |key: &str| self.resolve_config_type(key);
            let loaders = crate::type_engine::resolver::Loaders {
                function_loader: Some(&function_loader_cl),
                constant_loader: Some(&constant_loader_cl),
                config_resolver: Some(&config_resolver),
            };
            crate::type_engine::variable::forward_walk::build_diagnostic_scopes(
                content,
                &ctx.file.classes,
                &class_loader,
                loaders,
                Some(&self.resolved_class_cache),
            );
        }

        if let Some(ctx) = &file_ctx {
            self.collect_unknown_class_diagnostics_with_context(ctx, uri_str, content, out);
        }
        self.collect_class_case_mismatch_diagnostics(uri_str, content, out);
        if let Some(ctx) = &file_ctx {
            self.collect_unknown_member_diagnostics_with_context(ctx, uri_str, content, out);
            self.collect_unknown_function_diagnostics_with_context(ctx, uri_str, content, out);
        }
        // NOTE: unresolved_member_access diagnostics are now emitted
        // inside collect_unknown_member_diagnostics (in the Untyped arm)
        // to avoid a second full walk with duplicate type resolution.
        self.collect_argument_count_diagnostics(uri_str, content, out);
        self.collect_type_mismatch_diagnostics(uri_str, content, out);
        if let Some(ctx) = &file_ctx {
            self.collect_implementation_error_diagnostics_with_context(ctx, uri_str, content, out);
            self.collect_deprecated_diagnostics_with_context(ctx, uri_str, content, out);
        }
        self.collect_undefined_variable_diagnostics(uri_str, content, out);
        self.collect_match_type_diagnostics(uri_str, content, out);
        if let Some(ctx) = &file_ctx {
            self.collect_invalid_class_kind_diagnostics_with_context(ctx, uri_str, content, out);
            self.collect_enum_error_diagnostics_with_context(ctx, uri_str, content, out);
            self.collect_incompatible_override_diagnostics_with_context(ctx, uri_str, content, out);
        }
        let is_laravel = self.resolved_class_cache.read().is_laravel();
        if is_laravel {
            self.collect_invalid_laravel_string_key_diagnostics(uri_str, content, out);
            self.collect_invalid_command_param_diagnostics(uri_str, content, out);
        }
    }

    /// Emit a warning for each `$this->argument('x')` / `$this->option('x')`
    /// whose name is not a parameter of the enclosing command's `$signature`.
    fn collect_invalid_command_param_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        use crate::symbol_map::SymbolKind;

        // (name, is_option, start, end) for each own-param span.
        let spans: Vec<(String, bool, u32, u32)> = {
            let maps = self.symbol_maps.read();
            let Some(symbol_map) = maps.get(uri) else {
                return;
            };
            symbol_map
                .spans
                .iter()
                .filter_map(|span| {
                    if let SymbolKind::CommandOwnParam { name, is_option } = &span.kind {
                        Some((name.clone(), *is_option, span.start, span.end))
                    } else {
                        None
                    }
                })
                .collect()
        };
        if spans.is_empty() {
            return;
        }

        for (name, is_option, start, end) in &spans {
            // Resolve the enclosing command signature at the span offset.  If
            // the class declares no `$signature` (e.g. a `$name`-only or
            // dynamically-built command), skip — there is nothing to validate.
            let Some(signature) = crate::virtual_members::laravel::command_signature_at_offset(
                content,
                *start as usize,
            ) else {
                continue;
            };
            let known = if *is_option {
                signature.option(name).is_some()
            } else {
                signature.argument(name).is_some()
            };
            if !known
                && let Some(range) =
                    offset_range_to_lsp_range(content, *start as usize, *end as usize)
            {
                let label = if *is_option { "option" } else { "argument" };
                out.push(helpers::make_diagnostic(
                    range,
                    DiagnosticSeverity::WARNING,
                    "invalid_command_parameter",
                    format!("Unknown command {}: '{}'", label, name),
                ));
            }
        }
    }

    /// Emit a warning for each `LaravelStringKey` span whose key does
    /// not resolve to any declaration (typo in route name, config key,
    /// view name, or translation key).
    fn collect_invalid_laravel_string_key_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        use crate::symbol_map::{LaravelStringKind, SymbolKind};
        use std::collections::HashSet;

        // Extract the LaravelStringKey spans we need and determine which
        // kinds are present, then DROP the read lock before calling
        // enumeration functions.  Those functions call
        // `user_file_symbol_maps()` → `ensure_workspace_indexed()` →
        // `parse_files_parallel()` → `update_ast()` which acquires a
        // WRITE lock on `symbol_maps`.  Holding a read lock here while
        // that write is attempted would deadlock.
        let mut has_route = false;
        let mut has_config = false;
        let mut has_view = false;
        let mut has_trans = false;
        let mut has_command = false;
        let mut has_morph_alias = false;
        let key_spans: Vec<(LaravelStringKind, String, u32, u32)> = {
            let maps = self.symbol_maps.read();
            let Some(symbol_map) = maps.get(uri) else {
                return;
            };
            symbol_map
                .spans
                .iter()
                .filter_map(|span| {
                    if let SymbolKind::LaravelStringKey { kind, key } = &span.kind {
                        match kind {
                            LaravelStringKind::Route => has_route = true,
                            LaravelStringKind::Config => has_config = true,
                            LaravelStringKind::View => has_view = true,
                            LaravelStringKind::Trans => has_trans = true,
                            LaravelStringKind::Command => has_command = true,
                            LaravelStringKind::MorphAlias => has_morph_alias = true,
                        }
                        Some((kind.clone(), key.clone(), span.start, span.end))
                    } else {
                        None
                    }
                })
                .collect()
            // `maps` read lock is dropped here.
        };

        if !has_route && !has_config && !has_view && !has_trans && !has_command && !has_morph_alias
        {
            return;
        }

        // Enumerate valid keys once per kind (lazy), using the cached
        // enumerations.  Safe to call now that the `symbol_maps` read
        // lock has been released.
        let route_keys: HashSet<String> = if has_route {
            self.cached_routes()
                .iter()
                .map(|route| route.name.clone())
                .collect()
        } else {
            HashSet::new()
        };
        let config_keys: HashSet<String> = if has_config {
            self.cached_config_keys().into_iter().collect()
        } else {
            HashSet::new()
        };
        let view_keys: HashSet<String> = if has_view {
            self.cached_view_names().into_iter().collect()
        } else {
            HashSet::new()
        };
        let trans_keys: HashSet<String> = if has_trans {
            self.cached_trans_keys().into_iter().collect()
        } else {
            HashSet::new()
        };
        let command_names: HashSet<String> = if has_command {
            self.laravel_commands
                .read()
                .all_names()
                .into_iter()
                .collect()
        } else {
            HashSet::new()
        };
        // A morph alias is only checkable when the project calls
        // `Relation::enforceMorphMap()` / `requireMorphMap()`.  Without that,
        // an unmapped model still morphs under its class name, so the set of
        // valid `*_type` values is open and an unknown alias proves nothing.
        let morph_aliases: Option<HashSet<String>> = if has_morph_alias {
            let index = self.laravel_morph_map.read();
            index
                .is_enforced()
                .then(|| index.all_aliases().into_iter().collect())
        } else {
            None
        };

        for (kind, key, start, end) in &key_spans {
            let (valid, label, code) = match kind {
                LaravelStringKind::Route => {
                    (route_keys.contains(key), "route", "invalid_laravel_route")
                }
                LaravelStringKind::Config => {
                    // Config keys may be partial prefixes (e.g. `config('app')`)
                    // which are valid even without a direct match.
                    let valid = config_keys.contains(key)
                        || config_keys
                            .iter()
                            .any(|k| k.starts_with(&format!("{}.", key)));
                    (valid, "config key", "invalid_laravel_config")
                }
                LaravelStringKind::View => {
                    (view_keys.contains(key), "view", "invalid_laravel_view")
                }
                LaravelStringKind::Trans => {
                    // When no translation files are found at all, skip trans
                    // diagnostics entirely.  This avoids false positives in
                    // non-Laravel projects (WordPress, GetText) that also use
                    // `__()` or `trans()` as function names.
                    if trans_keys.is_empty() {
                        continue;
                    }
                    let valid = trans_keys.contains(key)
                        || trans_keys
                            .iter()
                            .any(|k| k.starts_with(&format!("{}.", key)));
                    (valid, "translation key", "invalid_laravel_trans")
                }
                LaravelStringKind::Command => {
                    // When no commands were indexed at all, skip command
                    // diagnostics entirely.  The scan is heuristic (it relies
                    // on the `*Command` naming convention), so an empty index
                    // likely means discovery failed rather than that every
                    // referenced command is invalid.
                    if command_names.is_empty() {
                        continue;
                    }
                    (
                        command_names.contains(key),
                        "command",
                        "invalid_laravel_command",
                    )
                }
                LaravelStringKind::MorphAlias => {
                    let Some(aliases) = &morph_aliases else {
                        continue;
                    };
                    (
                        aliases.contains(key),
                        "morph type",
                        "invalid_laravel_morph_alias",
                    )
                }
            };
            if !valid
                && let Some(range) =
                    offset_range_to_lsp_range(content, *start as usize, *end as usize)
            {
                out.push(helpers::make_diagnostic(
                    range,
                    DiagnosticSeverity::WARNING,
                    code,
                    format!("Unknown {}: '{}'", label, key),
                ));
            }
        }
    }

    /// Collect all type mismatch diagnostics: argument types, return
    /// types, and property assignment types.
    ///
    /// This is a convenience entry point that groups the three
    /// `type_mismatch_*` collectors.  The individual methods remain
    /// available for selective use (e.g. in `analyse.rs`).
    pub fn collect_type_mismatch_diagnostics(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        self.collect_argument_type_diagnostics(uri_str, content, out);
        self.collect_return_type_diagnostics(uri_str, content, out);
        self.collect_property_type_diagnostics(uri_str, content, out);
    }
}

/// How long to wait after the last keystroke before publishing diagnostics.
const DIAGNOSTIC_DEBOUNCE_MS: u64 = 500;

impl Backend {
    /// Deliver diagnostics for a single file.
    ///
    /// Called from the background diagnostic worker after debouncing.
    ///
    /// **Phase 1 (instant):** Run fast collectors (syntax errors,
    /// deprecated, unused imports) and assemble them with *cached* slow
    /// and PHPStan results.  In push mode the merged set is published;
    /// in pull mode it is cached and a `workspace/diagnostic/refresh` is
    /// sent so the editor re-pulls.  Either way the editor shows
    /// strikethrough and dimming within milliseconds.
    ///
    /// **Phase 2 (background):** Compute slow diagnostics, rebuild the
    /// full set (fast + fresh slow + cached PHPStan), and deliver it the
    /// same way: push mode publishes the complete set, replacing the
    /// Phase 1 snapshot; pull mode caches it, bumps the `resultId`, and
    /// sends another `workspace/diagnostic/refresh`.
    pub(crate) async fn publish_diagnostics_for_file(&self, uri_str: &str, content: &str) {
        if self.should_skip_diagnostics(uri_str) {
            return;
        }

        // The native collectors are expensive (a full-file forward walk and
        // type resolution — hundreds of ms to several seconds on a large
        // file). They are pure CPU work and must never run inline on an async
        // worker: editors pull both `textDocument/diagnostic` and
        // `workspace/diagnostic` on every keystroke, and a handful of
        // concurrent inline computes will occupy every async runtime thread,
        // stalling delivery of completion and hover responses even though
        // those handlers themselves finished. Run them on the blocking pool.

        // ── Phase 1: collect and cache fast diagnostics ─────────────
        let fast_diagnostics = {
            let backend = self.clone_for_blocking();
            let uri = uri_str.to_string();
            let content = content.to_string();
            crate::server::run_blocking_cancel_safe(move || {
                let mut out = Vec::new();
                let effective_owned = backend.blade_virtual_content.read().get(&uri).cloned();
                let effective = effective_owned.as_deref().unwrap_or(&content);
                backend.collect_fast_diagnostics(&uri, effective, &mut out);
                out
            })
            .await
            .unwrap_or_default()
        };

        {
            let mut cache = self.diag.last_fast.lock();
            cache.insert(uri_str.to_string(), fast_diagnostics.clone());
        }

        // Update the assembled cache immediately so pull-capable editors
        // can see fast diagnostics before the slower native passes finish.
        self.assemble_and_push(uri_str).await;
        if self.supports_pull_diagnostics.load(Ordering::Acquire)
            && let Some(client) = &self.client
        {
            let _ = client.workspace_diagnostic_refresh().await;
        }

        // ── Phase 2: compute and cache slow diagnostics ─────────────
        let slow_diagnostics = {
            let backend = self.clone_for_blocking();
            let uri = uri_str.to_string();
            let content = content.to_string();
            crate::server::run_blocking_cancel_safe(move || {
                // The resolved-class cache guard contains a thread-local raw
                // pointer; activate it on this blocking thread for the
                // duration of the synchronous collection only.
                let _cache_guard = crate::virtual_members::with_active_resolved_class_cache(
                    &backend.resolved_class_cache,
                );
                let mut out = Vec::new();
                let effective_owned = backend.blade_virtual_content.read().get(&uri).cloned();
                let effective = effective_owned.as_deref().unwrap_or(&content);
                backend.collect_slow_diagnostics(&uri, effective, &mut out);
                out
            })
            .await
            .unwrap_or_default()
        };

        {
            let mut cache = self.diag.last_slow.lock();
            cache.insert(uri_str.to_string(), slow_diagnostics);
        }

        // Update again with fresh slow results merged in.
        self.assemble_and_push(uri_str).await;
        if self.supports_pull_diagnostics.load(Ordering::Acquire)
            && let Some(client) = &self.client
        {
            let _ = client.workspace_diagnostic_refresh().await;
        }
    }

    /// Assemble diagnostics from all per-source caches for a URI and
    /// deliver them to the editor.
    ///
    /// Every source (fast, slow, PHPStan, PHPCS, Mago lint, Mago
    /// analyze) caches its results independently.  This helper merges
    /// them into one set, deduplicates, and filters suppressions.
    ///
    /// **Push mode:** The merged set is published via
    /// `textDocument/publishDiagnostics`.
    ///
    /// **Pull mode:** Nothing is pushed.  The full merged set is cached
    /// in `diag_last_full` with a bumped `resultId` so the next pull
    /// response returns it; the caller triggers
    /// `workspace/diagnostic/refresh` so the editor re-pulls.  Editors
    /// that support pull diagnostics merge pushed and pulled sets
    /// additively, so pushing anything here would duplicate native
    /// diagnostics.
    pub(crate) async fn assemble_and_push(&self, uri_str: &str) {
        let client = match &self.client {
            Some(c) => c,
            None => return,
        };

        let uri = match uri_str.parse::<Url>() {
            Ok(u) => u,
            Err(_) => return,
        };

        // ── Read all per-source caches ──────────────────────────────
        let mut full = Vec::new();

        {
            let cache = self.diag.last_fast.lock();
            if let Some(fast) = cache.get(uri_str) {
                full.extend(fast.iter().cloned());
            }
        }
        {
            let cache = self.diag.last_slow.lock();
            if let Some(slow) = cache.get(uri_str) {
                full.extend(slow.iter().cloned());
            }
        }

        let phpstan_before: Vec<Diagnostic> = {
            let cache = self.phpstan_tool.last_diags.lock();
            cache.get(uri_str).cloned().unwrap_or_default()
        };

        // Eagerly prune stale PHPStan diagnostics against current
        // file content (e.g. @throws tag added/removed, @phpstan-ignore
        // comment added).
        if !phpstan_before.is_empty() {
            let content: Option<Arc<String>> = self.open_files.read().get(uri_str).cloned();
            let filtered: Vec<Diagnostic> = phpstan_before
                .iter()
                .filter(|d| {
                    if let Some(ref text) = content {
                        !stale::is_stale_phpstan_diagnostic(d, text)
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();
            if filtered.len() != phpstan_before.len() {
                let mut cache = self.phpstan_tool.last_diags.lock();
                cache.insert(uri_str.to_string(), filtered.clone());
            }
            full.extend(filtered);
        }

        {
            let cache = self.phpcs_tool.last_diags.lock();
            if let Some(phpcs_diags) = cache.get(uri_str) {
                full.extend(phpcs_diags.iter().cloned());
            }
        }
        {
            let cache = self.mago_lint_tool.last_diags.lock();
            if let Some(mago_diags) = cache.get(uri_str) {
                full.extend(mago_diags.iter().cloned());
            }
        }
        {
            let cache = self.mago_analyze_tool.last_diags.lock();
            if let Some(mago_diags) = cache.get(uri_str) {
                full.extend(mago_diags.iter().cloned());
            }
        }

        // ── Suppress imprecise overlaps and filter ──────────────────
        suppression::suppress_imprecise_overlaps(&mut full);
        let mut full = self.filter_suppressed(full);

        // ── Apply @phpantom-ignore comment suppression ─────────────
        {
            let content: Option<Arc<String>> = self.open_files.read().get(uri_str).cloned();
            if let Some(ref text) = content {
                suppression::filter_ignored_by_comment(&mut full, text);
            }
        }

        // ── Apply [[diagnostics.ignore]] config rules ────────────────
        {
            let rules = ignore_rules::compile_ignore_rules(
                &self.workspace.config.lock().diagnostics.ignore,
            );
            if !rules.is_empty()
                && let Ok(file_path) = uri.to_file_path()
                && let Some(root) = self.workspace.workspace_root.read().clone()
            {
                let relative_path = file_path
                    .strip_prefix(&root)
                    .unwrap_or(&file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                ignore_rules::filter_ignored_by_config(&mut full, &relative_path, &rules);
            }
        }

        // If suppression removed any full-line PHPStan diagnostics
        // (because a precise native diagnostic covers the same line),
        // prune them from the PHPStan cache too so they don't resurface.
        if !phpstan_before.is_empty() {
            let pruned: Vec<Diagnostic> = phpstan_before
                .into_iter()
                .filter(|d| full.iter().any(|f| f.range == d.range))
                .collect();
            let mut cache = self.phpstan_tool.last_diags.lock();
            cache.insert(uri_str.to_string(), pruned);
        }

        let pull_mode = self.supports_pull_diagnostics.load(Ordering::Acquire);

        if pull_mode {
            // ── Pull mode ───────────────────────────────────────────
            // Pull-capable clients use `textDocument/diagnostic` as the
            // single source of truth for native diagnostics.  We only cache
            // the merged set here; the caller triggers
            // `workspace/diagnostic/refresh` so the editor re-pulls.  This
            // avoids duplicate diagnostics in clients that keep pushed and
            // pulled diagnostics in separate namespaces.
            {
                let mut cache = self.diag.last_full.lock();
                cache.insert(uri_str.to_string(), full);
            }
            {
                let mut ids = self.diag.result_ids.lock();
                let id = ids.entry(uri_str.to_string()).or_insert(0);
                *id += 1;
            }
        } else {
            // ── Push mode ───────────────────────────────────────────
            client.publish_diagnostics(uri, full, None).await;
        }
    }

    /// Notify the diagnostic system that a file needs fresh diagnostics.
    ///
    /// **Push mode:** Queues the file for the debounced background
    /// diagnostic worker and schedules external tool runs.
    ///
    /// **Pull mode:** Only schedules external tool runs (PHPStan,
    /// PHPCS, Mago).  Native diagnostic computation is deferred until
    /// the editor sends a `textDocument/diagnostic` pull request, which
    /// triggers [`trigger_diagnostics_for_pull`].
    ///
    /// This returns immediately — all diagnostic computation happens
    /// in the background so that completion, hover, and signature help
    /// are never blocked.
    pub(crate) fn schedule_diagnostics(&self, uri: String) {
        // Don't schedule diagnostics before initialization is complete.
        // Files opened during startup will be diagnosed once
        // `initialized` sets `init_complete` and re-schedules them.
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        // In pull mode we deliberately KEEP the previously cached full set.
        // The pull handler no longer computes on the request path (that
        // wedged the transport — see `trigger_diagnostics_for_pull`), so the
        // editor would otherwise flicker to empty between an edit and the
        // background recompute. Keeping the stale set means a pull returns
        // the old diagnostics until the worker recomputes, bumps the
        // resultId, and requests a refresh, which makes the editor re-pull
        // the fresh set. The resultId is bumped only by `assemble_and_push`,
        // so the "unchanged" fast-path stays correct.

        // Both modes: queue for the debounced background worker.
        // In push mode the worker pushes the full assembled set.
        // In pull mode the worker pushes only fast diagnostics and
        // caches the full set in `diag_last_full` for pull responses.
        {
            let mut pending = self.diag.pending_uris.lock();
            pending.insert(uri.clone());
        }
        self.diag.version.fetch_add(1, Ordering::Release);
        self.diag.notify.notify_one();

        // External tools (PHPStan, PHPCS, Mago) are NOT scheduled here.
        // They are expensive (seconds per run) and would block save-
        // triggered runs due to the serialization guarantee (one process
        // at a time).
    }

    /// Schedule all external tool runs (PHPStan, PHPCS, Mago) for a
    /// single file.
    ///
    /// External tools are expensive (seconds per run) and at most one
    /// process runs at a time per tool, so triggering them on every
    /// keystroke would block save-triggered runs.
    pub(crate) fn schedule_external_diagnostics(&self, uri: String) {
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }
        self.schedule_phpstan(uri.clone());
        self.schedule_phpcs(uri.clone());
        self.schedule_mago_lint(uri.clone());
        self.schedule_mago_analyze(uri);
    }

    /// Invalidate diagnostics for the open files a save can affect.
    ///
    /// Diagnostics in other open files (unknown member, unknown class,
    /// deprecated usage, argument checks) can depend on the saved file, so
    /// they have to be recomputed — but only for the files that reference
    /// something the save changed.  [`open_files_affected_by_save`] works
    /// out which those are, and falls back to every open file when it
    /// cannot tell.  The saved file itself is excluded (it is already
    /// scheduled by the caller).
    ///
    /// **Push mode:** Queues those files for the background worker.
    ///
    /// **Pull mode:** Invalidates cached full diagnostics and sends
    /// `workspace/diagnostic/refresh` so the editor re-pulls.
    ///
    /// [`open_files_affected_by_save`]: Backend::open_files_affected_by_save
    pub(crate) fn schedule_diagnostics_for_open_files(&self, exclude_uri: &str) {
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        let pull_mode = self.supports_pull_diagnostics.load(Ordering::Acquire);

        let uris = self.open_files_affected_by_save(exclude_uri);
        if uris.is_empty() {
            return;
        }

        if pull_mode {
            // Invalidate cached full diagnostics so the next pull
            // triggers a fresh computation.  Do NOT remove resultIds —
            // see the comment in schedule_diagnostics for why.
            let mut cache = self.diag.last_full.lock();
            for uri in &uris {
                cache.remove(uri);
            }
        }

        // Both modes: queue all files for the debounced background worker.
        // In push mode the worker pushes the full assembled set.
        // In pull mode the worker pushes only fast diagnostics and
        // caches the full set in `diag_last_full` for pull responses.
        {
            let mut pending = self.diag.pending_uris.lock();
            pending.extend(uris);
        }
        self.diag.version.fetch_add(1, Ordering::Release);
        self.diag.notify.notify_one();
    }

    /// Compute native diagnostics for a single file (pull-mode path).
    ///
    /// Called directly from the pull handler (`textDocument/diagnostic`)
    /// when the cached full diagnostics are stale or missing.  Runs
    /// both fast and slow collectors synchronously (no debounce) and
    /// caches the results.  The pull handler reads `diag_last_full`
    /// after this returns.
    pub(crate) fn trigger_diagnostics_for_pull(&self, uri_str: &str) {
        // Don't compute diagnostics before initialization is complete.
        // The pull handler will return empty results; once `initialized`
        // finishes it schedules all open files which populates the cache.
        if !self.init_complete.load(Ordering::Acquire) {
            return;
        }

        if self.should_skip_diagnostics(uri_str) {
            return;
        }

        // NEVER compute on the request path. The native pass takes seconds on
        // a large file, and editors pull both `textDocument/diagnostic` and
        // `workspace/diagnostic` on every keystroke — far faster than the
        // compute drains. A synchronous pull holds a tower-lsp concurrency
        // slot for the whole compute, so a typing burst fills every slot and
        // wedges the transport (it can no longer even read `$/cancelRequest`).
        //
        // Instead, ensure the debounced background worker will (re)compute
        // this file off the async runtime; it caches the full set and
        // requests a workspace diagnostic refresh when done. This pull
        // returns whatever is currently cached (stale results are kept until
        // the refresh, so the editor never flickers to empty).
        {
            let mut pending = self.diag.pending_uris.lock();
            pending.insert(uri_str.to_string());
        }
        self.diag.notify.notify_one();
    }

    /// Long-lived background task that processes diagnostic requests.
    ///
    /// Active in both push and pull modes.  In push mode, the worker
    /// pushes the full assembled diagnostic set via
    /// `publishDiagnostics`.  In pull mode, it pushes only fast
    /// diagnostics and caches the full set in `diag_last_full` for
    /// the next pull response (see [`assemble_and_push`]).
    ///
    /// Spawned once during `initialized`.  Loops forever, waiting for
    /// [`schedule_diagnostics`](Self::schedule_diagnostics) to signal
    /// new work.  On each iteration:
    ///
    /// 1. Wait for a notification (new edit arrived).
    /// 2. Debounce: sleep [`DIAGNOSTIC_DEBOUNCE_MS`], then check
    ///    whether the version counter moved (more edits).  If so,
    ///    loop back to step 2.
    /// 3. Snapshot the pending URI and current file content.
    /// 4. Run the diagnostic collectors and publish results.
    /// 5. Loop back to step 1.
    ///
    /// Because there is exactly one instance of this task, at most one
    /// diagnostic pass runs at a time.  If edits arrive during step 4
    /// the version counter will have moved, and step 1 picks up
    /// immediately after step 4 finishes — giving the two-slot
    /// (one running + one pending) behaviour.
    pub(crate) async fn diagnostic_worker(&self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 1: wait for work ───────────────────────────────
            self.diag.notify.notified().await;

            if self.shutdown_flag.load(Ordering::Acquire) {
                return;
            }

            // ── Step 2: debounce ────────────────────────────────────
            loop {
                let version_before = self.diag.version.load(Ordering::Acquire);
                tokio::time::sleep(std::time::Duration::from_millis(DIAGNOSTIC_DEBOUNCE_MS)).await;
                let version_after = self.diag.version.load(Ordering::Acquire);
                if version_before == version_after {
                    // No new edits during the sleep — proceed.
                    break;
                }
                // More edits arrived — loop and debounce again.
            }

            // ── Step 3: snapshot all pending URIs ────────────────────
            let uris: std::collections::HashSet<String> = {
                let mut pending = self.diag.pending_uris.lock();
                std::mem::take(&mut *pending)
            };
            if uris.is_empty() {
                continue;
            }

            // ── Step 4: collect and publish for each URI ────────────
            // Snapshot content for each URI individually, releasing the
            // read lock before each async publish call so that
            // `did_change` is never blocked.
            for uri in &uris {
                let content = {
                    let files = self.open_files.read();
                    match files.get(uri) {
                        Some(c) => c.clone(),
                        None => continue,
                    }
                };
                self.publish_diagnostics_for_file(uri, &content).await;
            }
        }
    }

    /// Clear diagnostics for a file (e.g. on `did_close`).
    pub(crate) async fn clear_diagnostics_for_file(&self, uri_str: &str) {
        // Migrate the live external tool results into the workspace
        // diagnostics store before purging, so a closed file keeps its
        // PHPStan/PHPCS/Mago findings instead of losing them until the
        // next project-wide run.  Only meaningful once the background
        // workspace pass has started.
        if self
            .diag
            .workspace_diag_pass_started
            .load(Ordering::Acquire)
        {
            let migrations: [(&'static str, Vec<Diagnostic>); 4] = [
                (
                    "phpstan",
                    self.phpstan_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
                (
                    "phpcs",
                    self.phpcs_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
                (
                    "mago-lint",
                    self.mago_lint_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
                (
                    "mago-analyze",
                    self.mago_analyze_tool
                        .last_diags
                        .lock()
                        .get(uri_str)
                        .cloned()
                        .unwrap_or_default(),
                ),
            ];
            let mut ws = self.diag.workspace_diags.lock();
            for (source, diags) in migrations {
                ws.set_external_for_uri(source, uri_str, diags);
            }
        }

        // Remove all per-source caches so we don't leak memory.
        self.diag.last_fast.lock().remove(uri_str);
        self.diag.last_slow.lock().remove(uri_str);
        // Remove cached PHPStan, PHPCS, and Mago diagnostics too.
        self.phpstan_tool.last_diags.lock().remove(uri_str);
        self.phpcs_tool.last_diags.lock().remove(uri_str);
        self.mago_lint_tool.last_diags.lock().remove(uri_str);
        self.mago_analyze_tool.last_diags.lock().remove(uri_str);
        // Remove pull-diagnostic caches.
        self.diag.result_ids.lock().remove(uri_str);
        self.diag.last_full.lock().remove(uri_str);

        let client = match &self.client {
            Some(c) => c,
            None => return,
        };

        let uri = match uri_str.parse::<Url>() {
            Ok(u) => u,
            Err(_) => return,
        };

        // Always push empty diagnostics to clear any Phase 1 snapshot.
        client.publish_diagnostics(uri, Vec::new(), None).await;

        if self.supports_pull_diagnostics.load(Ordering::Acquire) {
            // Tell the editor to re-pull diagnostics.  We spawn this
            // as a detached task instead of awaiting it because
            // workspace_diagnostic_refresh is a server-to-client
            // *request* that blocks until the client responds.  When
            // the editor closes many files in a burst, each didClose
            // handler would await a response while the client is busy
            // sending more messages, deadlocking the tower-lsp
            // service loop.
            let client = client.clone();
            tokio::spawn(async move {
                let _ = client.workspace_diagnostic_refresh().await;
            });
        }

        // Recompute the file's workspace diagnostics from disk so the
        // closed file's entry reflects the saved state (the startup
        // snapshot may be stale after in-editor edits).
        if self
            .diag
            .workspace_diag_pass_started
            .load(Ordering::Acquire)
        {
            let backend = self.clone_for_diagnostic_worker();
            let uri = uri_str.to_string();
            tokio::spawn(async move {
                backend
                    .recompute_workspace_diags_for_closed_file(&uri)
                    .await;
            });
        }
    }
}

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
        let virtual_php_handle = self.blade_virtual_content.read();
        if let Some(virtual_php) = virtual_php_handle.get(uri)
            && let Some(map) = self.blade_source_maps.read().get(uri)
        {
            if start_byte > virtual_php.len() || end_byte > virtual_php.len() {
                return None;
            }

            let mut range =
                crate::text_position::byte_range_to_lsp_range(virtual_php, start_byte, end_byte);

            if range.start.line < crate::blade::PROLOGUE_LINES {
                // Diagnostic originates from the prologue (injected headers).
                // We skip these to avoid false positives on line 1 of Blade.
                return None;
            }

            range.start = map.php_to_blade(range.start);
            range.end = map.php_to_blade(range.end);

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
pub(crate) fn offset_range_to_lsp_range(
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
    use super::*;

    #[test]
    fn test_offset_range_to_lsp_range_ignores_prologue() {
        let backend = Backend::new_test();
        let uri = "file:///test.blade.php";
        let content = "Hello World";
        backend.update_ast(uri, content);

        // First 6 lines are prologue (including wrapper function declaration).
        let virtual_php = {
            let vc_handle = backend.blade_virtual_content.read();
            vc_handle
                .get(uri)
                .cloned()
                .expect("Virtual content should exist")
        };

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
        backend
            .supports_pull_diagnostics
            .store(true, Ordering::Release);

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

    /// Regression test: `collect_invalid_laravel_string_key_diagnostics`
    /// must not hold a `symbol_maps` read lock while calling enumeration
    /// functions that reach `ensure_workspace_indexed()` →
    /// `parse_files_parallel()` → `update_ast()` → `symbol_maps.write()`.
    ///
    /// Before the fix, this deadlocked because the read lock was held
    /// for the entire function body.  The fix extracts the needed spans
    /// into an owned `Vec` and drops the lock before enumerating keys.
    ///
    /// To trigger the deadlock path, we create a workspace with an
    /// unindexed PHP file so `ensure_workspace_indexed` must parse it
    /// (acquiring a write lock).  A 5-second timeout catches the
    /// deadlock as a test failure instead of an infinite hang.
    #[test]
    fn laravel_string_key_diagnostics_no_deadlock() {
        // Set up a temp workspace with an unindexed PHP file so that
        // ensure_workspace_indexed() will call parse_files_parallel()
        // which needs a write lock on symbol_maps.
        let tmp = std::env::temp_dir().join("phpantom_deadlock_test");
        let _ = std::fs::create_dir_all(&tmp);
        let unindexed_file = tmp.join("Unindexed.php");
        std::fs::write(&unindexed_file, "<?php\nclass Unindexed {}\n").unwrap();

        let backend = crate::Backend::new_test_with_workspace(tmp.clone(), vec![]);

        // Register the unindexed file in fqn_uri_index so
        // ensure_workspace_indexed Phase 1 will try to parse it.
        let unindexed_uri = format!("file://{}", unindexed_file.to_str().unwrap());
        backend
            .symbols
            .fqn_uri_index
            .write()
            .insert("Unindexed".to_string(), unindexed_uri);

        // Parse a file with Laravel string key spans.
        let uri = "file:///app/Http/test.php";
        let php = "<?php\nconfig('app.name');\nroute('home');\n";
        backend.update_ast(uri, php);

        // Run the diagnostics in a thread with a timeout so a deadlock
        // is caught as a failure rather than an infinite hang.
        let (tx, rx) = std::sync::mpsc::channel();
        let backend = std::sync::Arc::new(backend);
        let bc = std::sync::Arc::clone(&backend);
        std::thread::spawn(move || {
            let mut out = Vec::new();
            bc.collect_slow_diagnostics(uri, php, &mut out);
            let _ = tx.send(out);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(_diags) => { /* success — no deadlock */ }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!(
                    "collect_slow_diagnostics deadlocked: symbol_maps read lock \
                     was likely held while enumeration functions tried to write"
                );
            }
            Err(e) => panic!("collect_slow_diagnostics failed: {:?}", e),
        }

        // Clean up.
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
