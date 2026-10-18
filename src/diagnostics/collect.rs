//! Running the diagnostic collectors over one file: the fast pass, and the
//! slow pass over the whole file or the regions an edit touched.

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::type_engine::resolver::LendsLoaders;

use super::{helpers, incremental, offset_range_to_lsp_range};

/// Callback invoked after each Phase 2 collector in
/// [`Backend::collect_slow_diagnostics_observed`]: receives the
/// collector's name and how long it ran, and returns `false` to skip the
/// remaining collectors.
pub(crate) type SlowDiagnosticObserver<'a> =
    &'a mut dyn FnMut(&'static str, std::time::Duration) -> bool;

impl Backend {
    /// Returns `true` if the URI should be skipped for diagnostics
    /// (stub files only).  Vendor files are not skipped here: users
    /// working in monorepos or with `--prefer-source` packages
    /// legitimately edit vendor files, so the live per-file pass
    /// diagnoses them like any other open file.  The workspace-wide
    /// background pass excludes vendor files separately, via its own
    /// `vendor_uri_prefixes` filter.
    pub(super) fn should_skip_diagnostics(&self, uri_str: &str) -> bool {
        uri_str.starts_with("phpantom-stub://") || uri_str.starts_with("phpantom-stub-fn://")
    }

    /// Collect Phase 1 (fast) diagnostics: syntax errors, unused
    /// imports/variables, namespace/class-name mismatches, and
    /// docblock/native type-hint contradictions.  These are cheap — no type
    /// resolution.
    pub(crate) fn collect_fast_diagnostics(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let _lines = crate::text_position::PassLineTable::new(content);
        // Four of these parse the file.  Without a shared cache each would
        // parse it again, on every keystroke; the guard makes them reuse one
        // AST.  A nested guard is a no-op, so the workspace and analyze paths
        // that already hold one are unaffected.
        let _parse_guard = crate::parser::with_parse_cache(content);

        self.collect_syntax_error_diagnostics(uri_str, content, out);
        self.collect_unused_import_diagnostics(uri_str, content, out);
        self.collect_unused_variable_diagnostics(uri_str, content, out);
        self.collect_namespace_mismatch_diagnostics(uri_str, content, out);
        self.collect_class_name_mismatch_diagnostics(uri_str, content, out);
        self.collect_docblock_native_mismatch_diagnostics(uri_str, content, out);
        self.collect_unreachable_code_diagnostics(uri_str, content, out);
    }

    /// Collect Phase 2 (slow) diagnostics: unknown class/member/function,
    /// argument count, implementation errors, deprecated usage, type
    /// mismatches, and (in Laravel projects) route/config/view/
    /// translation/command name checks.  These require type resolution
    /// and are expensive.  See the module docs for the full list.
    pub fn collect_slow_diagnostics(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        self.collect_slow_diagnostics_observed(uri_str, content, out, None);
    }

    /// [`Self::collect_slow_diagnostics`], with an optional per-collector
    /// observer.
    ///
    /// The observer receives each collector's name and how long it ran,
    /// and returns `false` to skip the remaining collectors.  The
    /// `analyse` CLI uses it to attribute a slow or hanging file to a
    /// single collector and to abandon a file that blows its deadline.
    /// Having it share this collector list is what keeps the CLI from
    /// silently missing a diagnostic kind the LSP reports.
    pub(crate) fn collect_slow_diagnostics_observed(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
        observe: Option<SlowDiagnosticObserver<'_>>,
    ) {
        self.collect_slow_diagnostics_scoped(uri_str, content, out, observe, None);
    }

    /// [`Self::collect_slow_diagnostics`] for the functions and methods
    /// spanning `spans` only, whose bodies open at `targets`.
    ///
    /// Collectors skip the sites outside `spans` (see
    /// [`incremental::region`]), and the forward walk visits only the
    /// bodies holding `targets`.  A collector may still report outside
    /// `spans`; the caller drops those.
    pub(crate) fn collect_slow_diagnostics_in_region(
        &self,
        uri_str: &str,
        content: &str,
        spans: &[(u32, u32)],
        targets: &[u32],
        out: &mut Vec<Diagnostic>,
    ) {
        let _region = incremental::region::restrict_to(spans.to_vec());
        self.collect_slow_diagnostics_scoped(uri_str, content, out, None, Some(targets));
    }

    /// The slow pass behind [`Self::collect_slow_diagnostics_observed`] and
    /// [`Self::collect_slow_diagnostics_in_region`]: the whole file when
    /// `targets` is `None`, otherwise only the bodies holding `targets`.
    fn collect_slow_diagnostics_scoped(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
        observe: Option<SlowDiagnosticObserver<'_>>,
        targets: Option<&[u32]>,
    ) {
        // Where this pass's own diagnostics start, so the reachability
        // filter below only judges what the collectors add and leaves any
        // fast diagnostics the caller already collected.
        let slow_start = out.len();
        let _lines = crate::text_position::PassLineTable::new(content);
        // The collectors and the forward walk all parse the file; share one
        // AST between them, as the fast pass does.
        let _parse_guard = crate::parser::with_parse_cache(content);

        // ── Phase 2: forward-walked diagnostic scope cache ──────
        // Walk every function/method body in the file once with the
        // forward walker, recording scope snapshots at each statement
        // boundary.  All subsequent `resolve_variable_types` calls
        // from diagnostic collectors hit the cache (O(log N) lookup)
        // instead of doing a full backward scan per member-access
        // span.  This eliminates the O(N × depth × file_size) cost
        // that caused multi-minute analysis times on large files.
        //
        // Held here rather than around the collectors alone: the same
        // walk records which branches cannot run, and the filter below
        // reads those ranges once the collectors are done.
        let _scope_guard =
            crate::type_engine::variable::forward_walk::with_diagnostic_scope_cache();

        self.run_slow_collectors(uri_str, content, out, observe, targets);

        // ── Drop what a branch that cannot run reported ──────────────
        // The collectors each walk spans of their own with no notion of
        // control flow, so they judge a branch a decidable guard rules
        // out exactly as readily as live code.  The walk that built the
        // scope cache recorded those branches; nothing inside one is a
        // finding.
        let unreachable = crate::type_engine::variable::forward_walk::unreachable_ranges();
        if !unreachable.is_empty() {
            let dead: Vec<Range> = unreachable
                .iter()
                .filter_map(|&(start, end)| {
                    offset_range_to_lsp_range(content, start as usize, end as usize)
                })
                .collect();
            let mut i = slow_start;
            while i < out.len() {
                let start = out[i].range.start;
                if dead
                    .iter()
                    .any(|range| start >= range.start && start < range.end)
                {
                    out.remove(i);
                } else {
                    i += 1;
                }
            }
        }
    }

    /// Run every slow collector for `uri_str`, in order.
    ///
    /// Split out from [`Self::collect_slow_diagnostics_observed`] so the
    /// observer's "stop here" can return from the collector list while
    /// the reachability filter still runs on what was collected.
    fn run_slow_collectors(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
        mut observe: Option<SlowDiagnosticObserver<'_>>,
        targets: Option<&[u32]>,
    ) {
        // Activate the chain resolution cache so that all slow
        // diagnostic collectors share cached intermediate chain
        // prefix results (e.g. `$model->where(...)` resolved once
        // and reused by `$model->where(...)->whereNotNull(...)`).
        // This eliminates O(depth²) re-resolution of shared chain
        // prefixes across unknown_member, argument_count, type_error,
        // and deprecated collectors.
        let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();

        // Activate the type-engine resolvers.  This also brings up the
        // callable target cache, so the same method on the same class is
        // resolved at most once across all diagnostic collectors: for
        // example `Builder::where` is looked up once and reused for
        // every `$q->where(...)`, `$query->where(...)`, and
        // `Product::query()->where(...)` call site in the file.
        let _resolver_guard = crate::type_engine::call_resolution::activate_type_engine_caches();

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
            let class_loaders = self.resolved_class_loaders(&ctx.file);
            let source_class_loaders = self.class_loaders(&ctx.file);
            let owned_loaders = self.diagnostic_loaders(&ctx.file);
            match targets {
                None => crate::type_engine::variable::forward_walk::build_diagnostic_scopes(
                    content,
                    &ctx.file.classes,
                    &class_loaders.as_dyn(),
                    &source_class_loaders.as_dyn(),
                    &ctx.file.block_imports(),
                    Some(self),
                    owned_loaders.loaders(),
                    Some(&self.resolved_class_cache),
                ),
                Some(targets) => {
                    crate::type_engine::variable::forward_walk::build_diagnostic_scopes_for_offsets(
                        content,
                        &ctx.file.classes,
                        &class_loaders.as_dyn(),
                        &source_class_loaders.as_dyn(),
                        &ctx.file.block_imports(),
                        Some(self),
                        owned_loaders.loaders(),
                        Some(&self.resolved_class_cache),
                        targets,
                    )
                }
            }
        }

        // Run one collector, reporting its name and duration to the
        // observer and returning early when the observer asks to stop.
        macro_rules! step {
            ($name:literal, $call:expr) => {
                match observe.as_mut() {
                    Some(obs) => {
                        let t0 = std::time::Instant::now();
                        $call;
                        if !obs($name, t0.elapsed()) {
                            return;
                        }
                    }
                    None => $call,
                }
            };
        }

        if let Some(ctx) = &file_ctx {
            step!(
                "unknown_class",
                self.collect_unknown_class_diagnostics_with_context(ctx, uri_str, content, out)
            );
        }
        step!(
            "class_case_mismatch",
            self.collect_class_case_mismatch_diagnostics(uri_str, content, out)
        );
        if let Some(ctx) = &file_ctx {
            // unresolved_member_access diagnostics are emitted inside
            // collect_unknown_member_diagnostics (in the Untyped arm) to
            // avoid a second full walk with duplicate type resolution.
            step!(
                "unknown_member",
                self.collect_unknown_member_diagnostics_with_context(ctx, uri_str, content, out)
            );
            step!(
                "unknown_function",
                self.collect_unknown_function_diagnostics_with_context(ctx, uri_str, content, out)
            );
        }
        step!(
            "argument_count_mismatch",
            self.collect_argument_count_diagnostics(uri_str, content, out)
        );
        step!(
            "type_mismatch_argument",
            self.collect_argument_type_diagnostics(uri_str, content, out)
        );
        step!(
            "type_mismatch_return",
            self.collect_return_type_diagnostics(uri_str, content, out)
        );
        step!(
            "type_mismatch_property",
            self.collect_property_type_diagnostics(uri_str, content, out)
        );
        if let Some(ctx) = &file_ctx {
            step!(
                "invalid_readonly_write",
                self.collect_readonly_write_diagnostics_with_context(ctx, uri_str, content, out)
            );
            step!(
                "missing_implementation",
                self.collect_implementation_error_diagnostics_with_context(
                    ctx, uri_str, content, out
                )
            );
            step!(
                "incompatible_trait_property",
                self.collect_trait_property_conflict_diagnostics_with_context(
                    ctx, uri_str, content, out
                )
            );
            step!(
                "deprecated_usage",
                self.collect_deprecated_diagnostics_with_context(ctx, uri_str, content, out)
            );
        }
        step!(
            "unknown_variable",
            self.collect_undefined_variable_diagnostics(uri_str, content, out)
        );
        step!(
            "match_type_mismatch",
            self.collect_match_type_diagnostics(uri_str, content, out)
        );
        step!(
            "unknown_offset",
            self.collect_destructuring_offset_diagnostics(uri_str, content, out)
        );
        if let Some(ctx) = &file_ctx {
            step!(
                "invalid_class_kind",
                self.collect_invalid_class_kind_diagnostics_with_context(
                    ctx, uri_str, content, out
                )
            );
            step!(
                "enum_error",
                self.collect_enum_error_diagnostics_with_context(ctx, uri_str, content, out)
            );
            step!(
                "incompatible_override",
                self.collect_incompatible_override_diagnostics_with_context(
                    ctx, uri_str, content, out
                )
            );
        }
        let is_laravel = self.resolved_class_cache.read().is_laravel();
        if is_laravel {
            step!(
                "invalid_laravel_string_key",
                self.collect_invalid_laravel_string_key_diagnostics(uri_str, content, out)
            );
            step!(
                "invalid_command_parameter",
                self.collect_invalid_command_param_diagnostics(uri_str, content, out)
            );
            step!(
                "blade_call_site",
                self.collect_blade_call_site_diagnostics(uri_str, content, out)
            );
            step!(
                "blade_signature",
                self.collect_blade_signature_diagnostics(uri_str, out)
            );
            step!(
                "blade_directive_balance",
                self.collect_blade_directive_diagnostics(uri_str, out)
            );
            step!(
                "blade_component_tag_balance",
                self.collect_blade_component_tag_diagnostics(uri_str, out)
            );
            step!(
                "blade_section",
                self.collect_blade_section_diagnostics(uri_str, out)
            );
        }
    }
}
