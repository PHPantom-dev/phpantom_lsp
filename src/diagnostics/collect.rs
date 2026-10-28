//! Running the diagnostic collectors over one file: the fast pass, and the
//! slow pass over the whole file or the regions an edit touched.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::type_engine::resolver::LendsLoaders;

use super::{helpers, incremental};

/// A slow diagnostic collector: appends what it finds in the file to the
/// list.
type SlowCollectFn =
    fn(&Backend, &helpers::FileDiagnosticContext, &str, &str, &mut Vec<Diagnostic>);

struct SlowCollector {
    /// The name the `analyse` CLI attributes the collector's time to.
    name: &'static str,
    collect: SlowCollectFn,
    /// The projects the collector runs in.
    runs_in: Framework,
    /// Whether a finding can depend on any function body in the file, so
    /// that an edit to one body can change a diagnostic outside it.  Such
    /// a collector reports under its own name as the diagnostic code, and
    /// a partial pass runs it over the whole file instead of the edited
    /// region (see [`Backend::collect_slow_diagnostics_reading_every_body`]).
    reads_every_body: bool,
}

/// The projects a collector has anything to report in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Framework {
    Any,
    Laravel,
    /// Laravel, Symfony, Twig, or Yii, whose string keys share one check.
    StringKeys,
}

const fn always(name: &'static str, collect: SlowCollectFn) -> SlowCollector {
    SlowCollector {
        name,
        collect,
        runs_in: Framework::Any,
        reads_every_body: false,
    }
}

const fn laravel(name: &'static str, collect: SlowCollectFn) -> SlowCollector {
    SlowCollector {
        name,
        collect,
        runs_in: Framework::Laravel,
        reads_every_body: false,
    }
}

/// Every slow diagnostic collector, in the order they run.
static SLOW_COLLECTORS: &[SlowCollector] = &[
    always(
        "unknown_class",
        Backend::collect_unknown_class_diagnostics_with_context,
    ),
    always(
        "class_case_mismatch",
        Backend::collect_class_case_mismatch_diagnostics_with_context,
    ),
    // unresolved_member_access diagnostics are emitted inside
    // collect_unknown_member_diagnostics (in the Untyped arm) to avoid a
    // second full walk with duplicate type resolution.
    always(
        "unknown_member",
        Backend::collect_unknown_member_diagnostics_with_context,
    ),
    always(
        "unknown_function",
        Backend::collect_unknown_function_diagnostics_with_context,
    ),
    always(
        "argument_count_mismatch",
        Backend::collect_argument_count_diagnostics_with_context,
    ),
    always(
        "discarded_return_value",
        Backend::collect_discarded_result_diagnostics_with_context,
    ),
    always(
        "type_mismatch_argument",
        Backend::collect_argument_type_diagnostics_with_context,
    ),
    always(
        "type_mismatch_return",
        Backend::collect_return_type_diagnostics_with_context,
    ),
    always(
        "type_mismatch_property",
        Backend::collect_property_type_diagnostics_with_context,
    ),
    always(
        "invalid_readonly_write",
        Backend::collect_readonly_write_diagnostics_with_context,
    ),
    // Whether a property is ever written can be decided by any method.
    SlowCollector {
        name: "uninitialized_property",
        collect: Backend::collect_uninitialized_property_diagnostics_with_context,
        runs_in: Framework::Any,
        reads_every_body: true,
    },
    always(
        "missing_implementation",
        Backend::collect_implementation_error_diagnostics_with_context,
    ),
    always(
        "incompatible_trait_property",
        Backend::collect_trait_property_conflict_diagnostics_with_context,
    ),
    always(
        "deprecated_usage",
        Backend::collect_deprecated_diagnostics_with_context,
    ),
    always(
        "unknown_variable",
        Backend::collect_undefined_variable_diagnostics_with_context,
    ),
    always(
        "match_type_mismatch",
        Backend::collect_match_type_diagnostics_with_context,
    ),
    always(
        "unknown_offset",
        Backend::collect_destructuring_offset_diagnostics_with_context,
    ),
    always(
        "invalid_class_kind",
        Backend::collect_invalid_class_kind_diagnostics_with_context,
    ),
    always(
        "enum_error",
        Backend::collect_enum_error_diagnostics_with_context,
    ),
    always(
        "incompatible_override",
        Backend::collect_incompatible_override_diagnostics_with_context,
    ),
    always(
        "invalid_override_attribute",
        Backend::collect_override_attribute_diagnostics_with_context,
    ),
    always(
        "invalid_member_name",
        Backend::collect_dynamic_member_name_diagnostics_with_context,
    ),
    SlowCollector {
        name: "invalid_string_key",
        collect: Backend::collect_invalid_laravel_string_key_diagnostics,
        runs_in: Framework::StringKeys,
        reads_every_body: false,
    },
    laravel(
        "invalid_command_parameter",
        Backend::collect_invalid_command_param_diagnostics,
    ),
    laravel(
        "blade_call_site",
        Backend::collect_blade_call_site_diagnostics,
    ),
    laravel("blade_signature", |backend, ctx, uri, _, out| {
        backend.collect_blade_signature_diagnostics(ctx, uri, out)
    }),
    laravel("blade_directive_balance", |backend, _, uri, _, out| {
        backend.collect_blade_directive_diagnostics(uri, out)
    }),
    laravel("blade_component_tag_balance", |backend, _, uri, _, out| {
        backend.collect_blade_component_tag_diagnostics(uri, out)
    }),
    laravel("blade_section", |backend, _, uri, _, out| {
        backend.collect_blade_section_diagnostics(uri, out)
    }),
];

/// Callback invoked after each Phase 2 collector in
/// [`Backend::collect_slow_diagnostics_observed`]: receives the
/// collector's name and how long it ran, and returns `false` to skip the
/// remaining collectors.
pub(crate) type SlowDiagnosticObserver<'a> =
    &'a mut dyn FnMut(&'static str, std::time::Duration) -> bool;

impl Backend {
    /// Returns `true` if the URI should be skipped for diagnostics: stub
    /// files (embedded or under a stub path), and resource documents (Twig
    /// templates, YAML and XML files), which are not PHP.  Vendor files are
    /// not skipped here: users
    /// working in monorepos or with `--prefer-source` packages
    /// legitimately edit vendor files, so the live per-file pass
    /// diagnoses them like any other open file.  The workspace-wide
    /// background pass excludes vendor files separately, via its own
    /// `vendor_uri_prefixes` filter.
    pub(super) fn should_skip_diagnostics(&self, uri_str: &str) -> bool {
        crate::stubs::is_stub_uri(uri_str)
            || self.is_external_stub_uri(uri_str)
            || self.is_resource_uri(uri_str)
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

    /// Run the collectors whose findings can depend on any function body
    /// over the whole file.  A partial pass skips them in the edited
    /// region and calls this instead, after dropping what they reported
    /// last time (see [`reads_every_body`]).
    pub(crate) fn collect_slow_diagnostics_reading_every_body(
        &self,
        uri_str: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let _parse_guard = crate::parser::with_parse_cache(content);
        let ctx = helpers::FileDiagnosticContext::gather(self, uri_str);
        for collector in SLOW_COLLECTORS {
            if collector.reads_every_body && collector.runs_in == Framework::Any {
                (collector.collect)(self, &ctx, uri_str, content, out);
            }
        }
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
        // finding.  The collectors' ranges are already mapped back into a
        // template's own coordinates, so the dead ranges must be too.
        let unreachable = crate::type_engine::variable::forward_walk::scope_unreachable_ranges();
        if !unreachable.is_empty() {
            let dead: Vec<Range> = unreachable
                .iter()
                .filter_map(|&(start, end)| {
                    self.offset_range_to_lsp_range(uri_str, content, start as usize, end as usize)
                })
                .collect();
            let mut index = 0;
            out.retain(|diagnostic| {
                let keep = index < slow_start || {
                    let start = diagnostic.range.start;
                    !dead
                        .iter()
                        .any(|range| start >= range.start && start < range.end)
                };
                index += 1;
                keep
            });
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

        // ── Shared per-file snapshot for every collector ────────────
        // Built once and reused by the forward-walker warm-up below and by
        // every collector, so a single diagnostic pass reads the file's
        // state and `uri_classes_index` once instead of once per
        // collector, and all of them observe the same snapshot even if a
        // concurrent parse publishes a new one mid-pass.
        let ctx = helpers::FileDiagnosticContext::gather(self, uri_str);

        let class_loaders = self.resolved_class_loaders(&ctx.file);
        let source_class_loaders = self.class_loaders(&ctx.file);
        let owned_loaders = self.diagnostic_loaders(&ctx.file);
        match targets {
            None => crate::type_engine::variable::forward_walk::build_diagnostic_scopes(
                content,
                &ctx.file.classes,
                &class_loaders.as_dyn_resolved(),
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
                    &class_loaders.as_dyn_resolved(),
                    &source_class_loaders.as_dyn(),
                    &ctx.file.block_imports(),
                    Some(self),
                    owned_loaders.loaders(),
                    Some(&self.resolved_class_cache),
                    targets,
                )
            }
        }

        let is_laravel = self.resolved_class_cache.read().laravel().is_enabled();
        let has_string_keys = is_laravel
            || self.is_symfony_project()
            || self.is_twig_project()
            || self.is_yii_project();
        for collector in SLOW_COLLECTORS {
            let runs = match collector.runs_in {
                Framework::Any => true,
                Framework::Laravel => is_laravel,
                Framework::StringKeys => has_string_keys,
            };
            // A partial pass runs these over the whole file separately.
            if !runs || (collector.reads_every_body && targets.is_some()) {
                continue;
            }
            match observe.as_mut() {
                Some(observe) => {
                    let t0 = std::time::Instant::now();
                    (collector.collect)(self, &ctx, uri_str, content, out);
                    // The observer's "stop here" ends the collector list;
                    // the caller still filters what was collected.
                    if !observe(collector.name, t0.elapsed()) {
                        return;
                    }
                }
                None => (collector.collect)(self, &ctx, uri_str, content, out),
            }
        }
    }
}

/// Whether `diagnostic` came from a collector that reads every function
/// body in the file.
pub(crate) fn reads_every_body(diagnostic: &Diagnostic) -> bool {
    let Some(Code::String(code)) = &diagnostic.code else {
        return false;
    };
    SLOW_COLLECTORS
        .iter()
        .any(|collector| collector.reads_every_body && collector.name == code)
}
