//! `@deprecated` usage diagnostics.
//!
//! Walk the precomputed [`SymbolMap`] for a file and flag every reference
//! to a class, method, property, constant, or function that carries a
//! `@deprecated` PHPDoc tag or a `#[Deprecated]` attribute.
//!
//! Diagnostics use `Severity::Hint` with `DiagnosticTag::Deprecated`,
//! which renders as a subtle strikethrough in most editors — visible but
//! not noisy.  The message includes the deprecation reason when one is
//! provided in the tag (e.g. `@deprecated Use NewHelper instead`).
//!
//! Variable type resolution is cached per [`SubjectCacheKey`] so that
//! multiple member accesses on the same variable (e.g. `$user->getName()`
//! and `$user->getEmail()`) only trigger a single resolution pass instead
//! of re-parsing the file for each access.  The key is shared with the
//! unknown-member pass, which is what keeps a same-named variable in
//! another method (or another branch of an `instanceof` check) from
//! reusing this one's type.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::{ClassRefContext, SymbolKind};
use crate::type_engine::resolver::{
    CtxLoaders, ResolutionCtx, SubjectOutcome, resolve_subject_outcome,
};
use crate::types::AccessKind;
use crate::types::{ClassInfo, ClassLikeKind};
use crate::virtual_members::{ResolvedClassCache, resolve_class_fully_cached};

use super::helpers::{
    FileDiagnosticContext, find_enclosing_method_name, find_innermost_enclosing_class,
    make_tagged_diagnostic, resolve_to_fqn,
};
use super::subject_cache::SubjectCacheKey;

impl Backend {
    /// Collect `@deprecated` usage diagnostics for a single file.
    ///
    /// Appends diagnostics to `out`.  The caller is responsible for
    /// publishing them via `textDocument/publishDiagnostics`.
    pub fn collect_deprecated_diagnostics(
        &self,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        let Some(ctx) = FileDiagnosticContext::gather(self, uri) else {
            return;
        };
        self.collect_deprecated_diagnostics_with_context(&ctx, uri, content, out);
    }

    /// Same as [`Self::collect_deprecated_diagnostics`] but reuses an
    /// already-gathered [`FileDiagnosticContext`] instead of re-reading
    /// the per-file locks. Used by `collect_slow_diagnostics` so all
    /// slow collectors in the same pass share one consistent snapshot.
    pub(crate) fn collect_deprecated_diagnostics_with_context(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        // ── Parse cache for this diagnostic pass ────────────────────────
        // Each subject resolution below re-parses the file via
        // `with_parsed_program` unless a parse cache is active; one pass
        // over a file with many distinct subjects must parse it once,
        // not once per subject.
        let _parse_guard = crate::parser::with_parse_cache(content);

        // ── Chain resolution cache for this diagnostic pass ─────────────
        let _chain_guard = crate::type_engine::resolver::with_chain_resolution_cache();

        // Cache of resolved variable types, keyed the same way as the
        // unknown-member pass so that all member accesses guaranteed to
        // see the same type share a single resolution pass.  This turns
        // O(n * parse) into O(k * parse) where k is the number of
        // distinct subjects, not the number of member accesses.
        let mut var_type_cache: HashMap<SubjectCacheKey, Option<ClassInfo>> = HashMap::new();

        let symbol_map = &ctx.symbol_map;
        let file_resolved_names = &ctx.file.resolved_names;
        let file_use_map = &ctx.file.use_map;
        let file_namespace = &ctx.file.namespace;
        let local_classes = &ctx.file.classes;

        let class_loader = self.class_loader_with(local_classes, file_use_map, file_namespace);
        let function_loader =
            self.function_loader_with(file_resolved_names.as_deref(), file_use_map, file_namespace);
        let laravel_macro_this_resolver = self.laravel_macro_this_resolver(&class_loader);
        let cache = &self.resolved_class_cache;

        let subject_ctx = crate::type_engine::subject_resolution::SubjectResolutionCtx {
            local_classes,
            use_map: file_use_map,
            namespace: file_namespace,
            content,
            class_loader: &class_loader,
            backend: Some(self),
            function_loader: &function_loader,
        };

        // A map extracted from different text than `content` describes a
        // file this pass cannot report on: every offset it holds would
        // land somewhere else.
        let Some(source) = symbol_map.source(content) else {
            return;
        };

        // ── Walk every symbol span ──────────────────────────────────────
        for span in &symbol_map.spans {
            match &span.kind {
                // ── Class references (type hints, new Foo, extends, etc.) ─
                SymbolKind::ClassReference {
                    name,
                    is_fqn,
                    context,
                    ..
                } => {
                    // An import is not a usage: it only says which `Foo`
                    // the names below mean.  Whatever the file does with
                    // the class is flagged where it does it, and flagging
                    // the import as well puts a second marker on code the
                    // developer has already been told about.
                    if matches!(context, ClassRefContext::UseImport) {
                        continue;
                    }
                    // Prefer mago-names byte-offset lookup when available —
                    // it applies PHP's full name resolution rules.  Fall
                    // back to the legacy resolve_to_fqn helper otherwise.
                    let resolved_name = if *is_fqn {
                        name.to_string()
                    } else if let Some(rn) = file_resolved_names {
                        rn.get(span.start)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| resolve_to_fqn(name, file_use_map, file_namespace))
                    } else {
                        resolve_to_fqn(name, file_use_map, file_namespace)
                    };

                    if let Some(cls) = self.find_or_load_class(&resolved_name)
                        && let Some(msg) = &cls.deprecation_message
                        && !is_within_deprecated_scope(
                            self,
                            local_classes,
                            &class_loader,
                            cache,
                            content,
                            span.start,
                        )
                        && let Some(range) = self.offset_range_to_lsp_range(
                            uri,
                            content,
                            span.start as usize,
                            span.end as usize,
                        )
                    {
                        let class_fqn = cls.fqn();
                        out.push(deprecated_diagnostic(
                            range,
                            &class_fqn,
                            None,
                            msg,
                            &cls.see_refs,
                        ));
                    }
                }

                // ── Member accesses ($x->method(), Foo::CONST, etc.) ─────
                SymbolKind::MemberAccess {
                    subject_text,
                    member_name,
                    is_static,
                    is_method_call,
                    ..
                } => {
                    // Resolve the subject type to a class.
                    let subject_str = subject_text.as_str(source);
                    let base_class = resolve_subject_to_class_name(
                        subject_str,
                        *is_static,
                        span.start,
                        &subject_ctx,
                    )
                    .and_then(|name| self.find_or_load_class(&name))
                    .map(|arc| ClassInfo::clone(&arc));

                    // Fall back to variable type resolution for $var->member() calls.
                    // Use the per-variable cache to avoid re-parsing the
                    // file for every member access on the same variable.
                    let base_class = match base_class {
                        Some(c) => c,
                        None if subject_str.starts_with('$') => {
                            let enclosing_class =
                                find_innermost_enclosing_class(local_classes, span.start);

                            let access_kind = if *is_static {
                                AccessKind::DoubleColon
                            } else {
                                AccessKind::Arrow
                            };

                            let cache_key = SubjectCacheKey::build(
                                symbol_map,
                                enclosing_class,
                                subject_str.trim(),
                                access_kind,
                                span.start,
                            );

                            let cached = var_type_cache.entry(cache_key).or_insert_with(|| {
                                let rctx = ResolutionCtx {
                                    is_in_static_method: symbol_map.is_in_static_method(span.start),
                                    ..self.resolution_ctx_at(
                                        enclosing_class,
                                        local_classes,
                                        content,
                                        span.start,
                                        CtxLoaders::new(
                                            &class_loader,
                                            &function_loader,
                                            &laravel_macro_this_resolver,
                                        ),
                                    )
                                };

                                resolve_variable_subject(subject_str, access_kind, &rctx)
                            });

                            match cached {
                                Some(c) => c.clone(),
                                None => continue,
                            }
                        }
                        None => continue,
                    };

                    // Resolve with inheritance + virtual members so we find
                    // members from parent classes and traits too.
                    //
                    // Check the base_class directly first: when the base
                    // comes from variable resolution or call-chain return
                    // type inference, it may already carry model-specific
                    // members (e.g. Eloquent scope methods injected onto
                    // Builder<Model>).  The FQN-keyed cache cannot
                    // distinguish between generic instantiations, so a
                    // cached entry may lack these members.
                    let resolved = resolve_class_fully_cached(&base_class, &class_loader, cache);

                    if *is_method_call {
                        // Check method deprecation — try base_class first
                        // (preserves scope methods), fall back to resolved.
                        if let Some(method) = base_class
                            .get_method(member_name)
                            .or_else(|| resolved.get_method(member_name))
                            && let Some(msg) = &method.deprecation_message
                            && !is_within_deprecated_scope(
                                self,
                                local_classes,
                                &class_loader,
                                cache,
                                content,
                                span.start,
                            )
                            && let Some(range) = self.offset_range_to_lsp_range(
                                uri,
                                content,
                                span.start as usize,
                                span.end as usize,
                            )
                        {
                            let class_fqn = resolved.fqn();
                            out.push(deprecated_diagnostic(
                                range,
                                member_name,
                                Some(&class_fqn),
                                msg,
                                &method.see_refs,
                            ));
                        }
                    } else {
                        // Property or constant access — try base_class
                        // first (same rationale as above), fall back to
                        // resolved.
                        if let Some(prop) = base_class
                            .properties
                            .iter()
                            .find(|p| p.name == *member_name)
                            .or_else(|| resolved.properties.iter().find(|p| p.name == *member_name))
                            && let Some(msg) = &prop.deprecation_message
                            && !is_within_deprecated_scope(
                                self,
                                local_classes,
                                &class_loader,
                                cache,
                                content,
                                span.start,
                            )
                            && let Some(range) = self.offset_range_to_lsp_range(
                                uri,
                                content,
                                span.start as usize,
                                span.end as usize,
                            )
                        {
                            let class_fqn = resolved.fqn();
                            out.push(deprecated_diagnostic(
                                range,
                                member_name,
                                Some(&class_fqn),
                                msg,
                                &prop.see_refs,
                            ));
                            continue;
                        }

                        // Try constant (static access like Foo::BAR)
                        if *is_static
                            && let Some(constant) =
                                resolved.constants.iter().find(|c| c.name == *member_name)
                            && let Some(msg) = &constant.deprecation_message
                            && !is_within_deprecated_scope(
                                self,
                                local_classes,
                                &class_loader,
                                cache,
                                content,
                                span.start,
                            )
                            && let Some(range) = self.offset_range_to_lsp_range(
                                uri,
                                content,
                                span.start as usize,
                                span.end as usize,
                            )
                        {
                            let class_fqn = resolved.fqn();
                            out.push(deprecated_diagnostic(
                                range,
                                member_name,
                                Some(&class_fqn),
                                msg,
                                &constant.see_refs,
                            ));
                        }
                    }
                }

                // ── Standalone function calls ────────────────────────────
                SymbolKind::FunctionCall {
                    name,
                    is_definition,
                    ..
                } => {
                    // Skip the declaration site — only flag call sites.
                    if *is_definition {
                        continue;
                    }
                    if let Some(func_info) = self.resolve_function_name_at(
                        name,
                        file_resolved_names.as_deref(),
                        span.start,
                        file_use_map,
                        ctx.file.namespace_at(span.start),
                    ) && let Some(msg) = &func_info.deprecation_message
                        && !is_within_deprecated_scope(
                            self,
                            local_classes,
                            &class_loader,
                            cache,
                            content,
                            span.start,
                        )
                        && let Some(range) = self.offset_range_to_lsp_range(
                            uri,
                            content,
                            span.start as usize,
                            span.end as usize,
                        )
                    {
                        out.push(deprecated_diagnostic(
                            range,
                            name,
                            None,
                            msg,
                            &func_info.see_refs,
                        ));
                    }
                }

                // Other symbol kinds are not checked for deprecation.
                _ => {}
            }
        }
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Build a deprecated diagnostic.
fn deprecated_diagnostic(
    range: Range,
    symbol_name: &str,
    class_name: Option<&str>,
    deprecation_message: &str,
    see_refs: &[String],
) -> Diagnostic {
    let display = if let Some(cls) = class_name {
        format!("{}::{}", cls, symbol_name)
    } else {
        symbol_name.to_string()
    };

    // Combine the deprecation message with @see references so the
    // diagnostic tooltip includes pointers to replacement APIs.
    let full_message = if see_refs.is_empty() {
        deprecation_message.to_string()
    } else {
        let see_list = see_refs.join(", ");
        if deprecation_message.is_empty() {
            format!("See: {}", see_list)
        } else {
            format!("{} (see: {})", deprecation_message, see_list)
        }
    };

    let message = if full_message.is_empty() {
        format!("'{}' is deprecated", display)
    } else {
        format!("'{}' is deprecated: {}", display, full_message)
    };

    make_tagged_diagnostic(
        range,
        DiagnosticSeverity::HINT,
        "deprecated_usage",
        message,
        Some(DiagnosticTag::DEPRECATED),
    )
}

/// Whether `offset` sits inside a scope that PHPStan's own deprecation
/// rule treats as deprecated: the enclosing class/trait itself, or the
/// enclosing method.
///
/// The method check also covers an override that implements/overrides a
/// deprecated interface or parent method without a `@deprecated` tag of
/// its own — inheritance enrichment (`inheritance::enrichment`) already
/// propagates `deprecation_message` onto such overrides, so a plain
/// lookup on the resolved class is enough.
///
/// Mirrors `DefaultDeprecatedScopeResolver` in
/// phpstan/phpstan-deprecation-rules: deprecated code calling other
/// deprecated code isn't worth flagging (e.g. a `Type::hasProperty()`
/// override delegating to the same deprecated method on other `Type`
/// instances).
fn is_within_deprecated_scope(
    backend: &Backend,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    cache: &ResolvedClassCache,
    content: &str,
    offset: u32,
) -> bool {
    let Some(enclosing) = find_innermost_enclosing_class(local_classes, offset) else {
        return false;
    };
    if enclosing.deprecation_message.is_some() {
        return true;
    }

    let Some(method_name) = find_enclosing_method_name(content, offset) else {
        return false;
    };
    let resolved = resolve_class_fully_cached(enclosing, class_loader, cache);
    if resolved
        .get_method(&method_name)
        .is_some_and(|m| m.deprecation_message.is_some())
    {
        return true;
    }

    // A trait method never runs on the trait: PHP flattens it into every
    // class that uses it, so whether it is the deprecated implementation
    // of something is decided over there.  Read the bound all of its
    // users satisfy — the same view of `$this` the type engine takes
    // inside a trait body — and ask the question there.
    if enclosing.kind != ClassLikeKind::Trait {
        return false;
    }
    crate::type_engine::trait_context::trait_this_bounds(
        enclosing,
        local_classes,
        class_loader,
        Some(backend),
    )
    .iter()
    .any(|bound| {
        resolve_class_fully_cached(bound, class_loader, cache)
            .get_method(&method_name)
            .is_some_and(|m| m.deprecation_message.is_some())
    })
}

/// Resolve a member access subject text to a class FQN.
///
/// Handles:
/// - `self`, `static`, `parent` → resolve from enclosing class
/// - `ClassName` (static access) → resolve via use map
/// - chain subjects (`makeHelper()`, `Foo::create()`) → resolve their
///   return type through the shared chain resolver
/// - `$this` → resolve from enclosing class
/// - Other `$variable` subjects return `None` (resolved separately
///   by [`resolve_variable_subject`]).
fn resolve_subject_to_class_name(
    subject_text: &str,
    is_static: bool,
    access_offset: u32,
    ctx: &crate::type_engine::subject_resolution::SubjectResolutionCtx<'_>,
) -> Option<String> {
    let trimmed = subject_text.trim();

    // Variables are resolved separately by the full resolver pipeline.
    if trimmed.starts_with('$') && trimmed != "$this" {
        return None;
    }

    crate::type_engine::subject_resolution::resolve_subject_type(
        subject_text,
        is_static,
        access_offset,
        ctx,
    )
    .and_then(|t| t.top_level_class_names().into_iter().next())
}

/// Resolve a subject expression to a `ClassInfo` using the full resolver
/// pipeline ([`resolve_subject_outcome`]).
///
/// This handles both simple `$variable` subjects and complex expressions
/// like `$payment->getOrder()` or `$this->faker`.  The resolver uses
/// the diagnostic scope cache (when active) for variable lookups,
/// avoiding backward-scanner fallthroughs.
fn resolve_variable_subject(
    subject_text: &str,
    access_kind: AccessKind,
    rctx: &ResolutionCtx<'_>,
) -> Option<ClassInfo> {
    match resolve_subject_outcome(subject_text.trim(), access_kind, rctx) {
        SubjectOutcome::Resolved(classes) => {
            classes.into_iter().next().map(|arc| ClassInfo::clone(&arc))
        }
        _ => None,
    }
}
