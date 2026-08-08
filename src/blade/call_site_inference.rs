//! Call-site variable inference for Blade templates.
//!
//! For templates without a declared signature (`@bladestan-signature`
//! or plain `@var` docblocks), infer the variables a template receives
//! from the `view()` call sites that reference it: literal array keys,
//! `compact()` arguments, and `->with()` chains.  The inferred set is
//! injected into the template's virtual-PHP prologue as `@var`
//! docblock declarations (see `preprocess_with_vars`), so every
//! consumer — completion, hover, go-to-definition, and the
//! undefined-variable diagnostic — sees them through the ordinary
//! resolution pipeline.
//!
//! This is deliberately the lowest-priority source: an in-template
//! `@var` annotation shadows an injected one (it sits closer to every
//! use site in the backward docblock scan), and templates that declare
//! a signature are skipped entirely.  Types are "true for the callers
//! we found": multiple call sites union per variable, and dynamic view
//! names contribute nothing.

use std::collections::HashMap;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::literal::{Literal, LiteralString};
use mago_syntax::cst::sequence::TokenSeparatedSequence;
use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::symbol_map::{LaravelStringKind, SymbolKind, SymbolMap};
use crate::type_engine::resolver::{Loaders, VarResolutionCtx};
use crate::types::ClassInfo;

/// A variable passed to a template at one call site: the name (without
/// `$`) and the expression's resolved type.
type InferredVars = Vec<(String, PhpType)>;

/// The variables injected into one template's virtual-PHP prologue:
/// (name without `$`, docblock type string).
pub(crate) type InjectedVars = Vec<(String, String)>;

/// User files that render Blade views, with their symbol maps.  Shared
/// across a whole refresh pass so the workspace is walked once, not once
/// per template.
pub(crate) type ViewCallerSnapshot = Vec<(String, Arc<SymbolMap>)>;

impl Backend {
    /// Compute the variables to inject into a Blade template's virtual
    /// PHP, by scanning `view()` call sites across the workspace.
    ///
    /// Returns pairs of (variable name without `$`, docblock type
    /// string).  Empty when the template declares its own signature,
    /// when no call site references it, or when the template's view
    /// name cannot be derived from its path.
    pub(crate) fn infer_blade_call_site_vars(
        &self,
        uri: &str,
        blade_content: &str,
        shared: Option<&ViewCallerSnapshot>,
    ) -> InjectedVars {
        // Templates that declare any `@var` annotation manage their own
        // contract; injecting on top would fight the declared types.
        if has_var_docblock(blade_content) {
            return Vec::new();
        }
        let view_names = self.view_names_for_blade_uri(uri);
        if view_names.is_empty() {
            return Vec::new();
        }

        // Find every file whose symbol map contains a View string key
        // matching one of this template's names.
        let own_snapshot;
        let snapshot = match shared {
            Some(shared) => shared.as_slice(),
            None => {
                let keys: Vec<crate::reference_index::ReferenceIndexKey> = view_names
                    .iter()
                    .map(
                        |name| crate::reference_index::ReferenceIndexKey::LaravelString {
                            kind: LaravelStringKind::View,
                            key: name.clone(),
                        },
                    )
                    .collect();
                // Never trigger (or wait on) workspace indexing from here:
                // this runs while a Blade file is being opened or a
                // controller saved, and a keystroke must not pay for a
                // workspace walk.  Before the index is ready this scans
                // whatever is parsed; the post-index refresh pass picks up
                // call sites discovered later.
                own_snapshot = self.user_file_symbol_maps_for_reference_keys_nonblocking(&keys);
                own_snapshot.as_slice()
            }
        };

        // Union the variables from every call site, per name.
        let mut merged: HashMap<String, Vec<PhpType>> = HashMap::new();
        for (file_uri, symbol_map) in snapshot {
            // A template must not feed itself (`@include` spans inside
            // the template's own virtual PHP), and other templates'
            // `@include`s would recurse — skip Blade files entirely.
            if self.is_blade_file(file_uri) {
                continue;
            }
            let offsets: Vec<u32> = symbol_map
                .spans
                .iter()
                .filter_map(|span| match &span.kind {
                    SymbolKind::LaravelStringKey {
                        kind: LaravelStringKind::View,
                        key,
                        ..
                    } if view_names.iter().any(|n| n == key) => Some(span.start),
                    _ => None,
                })
                .collect();
            if offsets.is_empty() {
                continue;
            }
            let Some(content) = self.get_file_content(file_uri) else {
                continue;
            };
            for vars in self.extract_call_site_vars(file_uri, &content, &offsets) {
                for (name, ty) in vars {
                    merged.entry(name).or_default().push(ty);
                }
            }
        }

        if merged.is_empty() {
            return Vec::new();
        }

        let mut result: Vec<(String, String)> = merged
            .into_iter()
            .map(|(name, types)| {
                let mut unique: Vec<PhpType> = Vec::new();
                for ty in types {
                    if !unique.iter().any(|u| u.equivalent(&ty)) {
                        unique.push(ty);
                    }
                }
                let joined = if unique.len() == 1 {
                    unique.pop().unwrap()
                } else {
                    PhpType::union(unique)
                };
                (name, joined.to_string())
            })
            .collect();
        // Deterministic prologue ordering so re-preprocessing an
        // unchanged template produces identical virtual PHP.
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Re-run call-site inference for already-preprocessed Blade
    /// templates and re-parse the ones whose inferred variable set
    /// changed.
    ///
    /// Parse order is arbitrary: a template preprocessed before its
    /// controllers were indexed saw no call sites.  Run this after a
    /// pass that parses many files (workspace indexing, the analyse
    /// CLI's parse phase) or after a controller edit, so templates pick
    /// up call sites discovered since they were preprocessed.  Cheap
    /// for templates whose inference is unchanged (no re-parse).
    pub(crate) fn refresh_blade_call_site_inference(&self) {
        let blade_uris: Vec<String> = self.blade_virtual_content.read().keys().cloned().collect();
        if blade_uris.is_empty() {
            return;
        }
        // Snapshot the caller files once for the whole pass.  Letting each
        // template take its own snapshot walks every symbol map in the
        // workspace per template, which is quadratic in a project with
        // hundreds of templates.
        let shared = self.view_caller_snapshot();
        for uri in blade_uris {
            let Some(content) = self.get_file_content(&uri) else {
                continue;
            };
            self.reinfer_and_reparse_blade_with(&uri, &content, Some(&shared));
        }
    }

    /// Every parsed user file that renders at least one Blade view, with
    /// its symbol map.  Non-Blade only: a template must not feed itself
    /// (`@include` spans sit inside its own virtual PHP), and other
    /// templates' `@include`s would recurse.
    fn view_caller_snapshot(&self) -> ViewCallerSnapshot {
        let vendor_prefixes = self.workspace.vendor_uri_prefixes.lock().clone();
        let maps = self.symbol_maps.read();
        maps.iter()
            .filter(|(uri, map)| {
                !uri.starts_with("phpantom-stub://")
                    && !uri.starts_with("phpantom-stub-fn://")
                    && !vendor_prefixes.iter().any(|p| uri.starts_with(p.as_str()))
                    && !self.is_blade_file(uri)
                    && map.spans.iter().any(|span| {
                        matches!(
                            &span.kind,
                            SymbolKind::LaravelStringKey {
                                kind: LaravelStringKind::View,
                                ..
                            }
                        )
                    })
            })
            .map(|(uri, map)| (uri.clone(), Arc::clone(map)))
            .collect()
    }

    /// Re-infer one template on its own, taking its own caller snapshot.
    /// For the single-template triggers (opening a Blade file, saving a
    /// controller) rather than a bulk refresh pass.
    pub(crate) fn reinfer_and_reparse_blade(&self, uri: &str, content: &str) -> bool {
        self.reinfer_and_reparse_blade_with(uri, content, None)
    }

    /// Recompute one template's inferred variable set; when it differs
    /// from the cached set, overwrite the cache and re-parse the
    /// template (`update_ast` reads the cache, so it must be written
    /// first).  A missing cache entry counts as empty, matching what
    /// `update_ast` injects on a cache miss.
    fn reinfer_and_reparse_blade_with(
        &self,
        uri: &str,
        content: &str,
        shared: Option<&ViewCallerSnapshot>,
    ) -> bool {
        let fresh = self.infer_blade_call_site_vars(uri, content, shared);
        let unchanged = match self.blade_injected_vars.read().get(uri) {
            Some(prev) => *prev == fresh,
            None => fresh.is_empty(),
        };
        if unchanged {
            return false;
        }
        self.blade_injected_vars
            .write()
            .insert(uri.to_string(), fresh);
        self.update_ast(uri, content);
        true
    }

    /// Re-run call-site inference for the templates referenced by one
    /// caller file (after it was edited or re-indexed), so an updated
    /// `view()` call is reflected in the template without waiting for
    /// the template's own next parse.
    ///
    /// Only templates that are already preprocessed are refreshed; a
    /// template parsed for the first time later runs inference itself.
    pub(crate) fn refresh_blade_inference_for_caller(&self, caller_uri: &str) {
        if self.is_blade_file(caller_uri) {
            return;
        }
        let Some(map) = self.symbol_maps.read().get(caller_uri).cloned() else {
            return;
        };
        let mut names: Vec<&str> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::LaravelStringKey {
                    kind: LaravelStringKind::View,
                    key,
                    ..
                } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        if names.is_empty() {
            return;
        }
        names.sort_unstable();
        names.dedup();

        for name in names {
            for location in crate::virtual_members::laravel::resolve_laravel_string_key(
                self,
                &LaravelStringKind::View,
                name,
            ) {
                let template_uri = location.uri.to_string();
                if !self
                    .blade_virtual_content
                    .read()
                    .contains_key(&template_uri)
                {
                    continue;
                }
                let Some(content) = self.get_file_content(&template_uri) else {
                    continue;
                };
                if self.reinfer_and_reparse_blade(&template_uri, &content) {
                    self.schedule_diagnostics(template_uri);
                }
            }
        }
    }

    /// Derive the view names a Blade file is addressable by: one per
    /// configured view root that contains it, in dot notation, plus
    /// `namespace::name` forms for provider-registered directories.
    fn view_names_for_blade_uri(&self, uri: &str) -> Vec<String> {
        let Ok(url) = tower_lsp::lsp_types::Url::parse(uri) else {
            return Vec::new();
        };
        let Ok(path) = url.to_file_path() else {
            return Vec::new();
        };

        let mut names = Vec::new();
        let mut push_name = |rel: &std::path::Path, namespace: &str| {
            let rel_str = rel.to_string_lossy();
            let stripped = rel_str
                .strip_suffix(".blade.php")
                .or_else(|| rel_str.strip_suffix(".php"));
            if let Some(stem) = stripped {
                let name = stem.replace(['/', '\\'], ".");
                if namespace.is_empty() {
                    names.push(name);
                } else {
                    names.push(format!("{namespace}::{name}"));
                }
            }
        };

        // `path` came from a file URI and is absolute; a view root can
        // be relative when the workspace root was given relative (the
        // analyse CLI passes `--project-root` through as-is), so
        // canonicalize each root before comparing.
        for root in self.laravel_view_roots() {
            let root = root.canonicalize().unwrap_or(root);
            if let Ok(rel) = path.strip_prefix(&root) {
                push_name(rel, "");
            }
        }
        for res in &self.laravel_provider_resources.read().view_dirs {
            let res_path = res.path.canonicalize().unwrap_or_else(|_| res.path.clone());
            if let Ok(rel) = path.strip_prefix(&res_path) {
                push_name(rel, &res.namespace);
            }
        }
        names
    }

    /// Parse one caller file and extract the variables passed to the
    /// template at each `view('name', …)` span offset.
    ///
    /// `offsets` are the byte offsets of the view-name string contents
    /// (as recorded in the symbol map); a call site matches when its
    /// first argument's span contains one of them.
    fn extract_call_site_vars(
        &self,
        uri: &str,
        content: &str,
        offsets: &[u32],
    ) -> Vec<InferredVars> {
        let file_ctx = self.file_context(uri);
        let class_loader = self.class_loader(&file_ctx);
        let function_loader = self.function_loader(&file_ctx);
        let function_loader_cl = |name: &str, offset: u32| function_loader(name, offset);

        with_parsed_program(content, "blade_call_site_inference", |program, content| {
            let default_class = ClassInfo::default();

            // Collect the matching call expressions first, then resolve
            // types — both inside the closure so AST references never
            // outlive the arena.
            let mut collected: Vec<(u32, Vec<SiteEntry<'_, '_>>)> = Vec::new();
            let walker = ViewCallWalker { offsets };
            let mut ctx = CollectCtx {
                sites: &mut collected,
            };
            for stmt in program.statements.iter() {
                mago_syntax::walker::Walker::walk_statement(&walker, stmt, &mut ctx);
            }

            let mut result = Vec::new();
            for (site_offset, entries) in collected {
                let enclosing =
                    crate::class_lookup::find_class_at_offset(&file_ctx.classes, site_offset);
                let current_class = enclosing.unwrap_or(&default_class);
                let loaders = Loaders::with_function(Some(&function_loader_cl));
                let var_ctx = VarResolutionCtx {
                    var_name: "",
                    top_level_scope: None,
                    current_class,
                    all_classes: &file_ctx.classes,
                    content,
                    cursor_offset: site_offset,
                    class_loader: &class_loader,
                    backend: Some(self),
                    loaders,
                    resolved_class_cache: Some(&self.resolved_class_cache),
                    enclosing_return_type: None,
                    branch_aware: false,
                    match_arm_narrowing: HashMap::new(),
                    scope_var_resolver: None,
                };

                let mut vars: InferredVars = Vec::new();
                for entry in entries {
                    let (name, ty) = match entry {
                        SiteEntry::Expr(name, expr) => {
                            let ty = crate::type_engine::variable::foreach_resolution::resolve_expression_type(
                                expr, &var_ctx,
                            )
                            .unwrap_or_else(PhpType::mixed);
                            (name, ty)
                        }
                        SiteEntry::Variable(name) => {
                            let loaders = Loaders::with_function(Some(&function_loader_cl));
                            let ty = crate::type_engine::variable::resolution::resolve_variable_php_type(
                                &name,
                                content,
                                site_offset,
                                Some(current_class),
                                &file_ctx.classes,
                                &class_loader,
                                Some(self),
                                loaders,
                            )
                            .unwrap_or_else(PhpType::mixed);
                            (name, ty)
                        }
                    };
                    // Render FQNs so the injected `@var` resolves from the
                    // template's namespace-less context.
                    let ty = ty.resolve_names(&|name: &str| {
                        if let Some(cls) = class_loader(name) {
                            format!("\\{}", cls.fqn())
                        } else {
                            name.to_string()
                        }
                    });
                    vars.push((name, ty));
                }
                if !vars.is_empty() {
                    result.push(vars);
                }
            }
            result
        })
    }
}

// ─── AST walking ────────────────────────────────────────────────────────────

/// One variable passed at a call site: either the value expression
/// (array entry / `->with()` value) or, for `compact('name')`, the
/// same-named variable to resolve at the call-site offset.
enum SiteEntry<'ast, 'arena> {
    Expr(String, &'ast Expression<'arena>),
    Variable(String),
}

struct CollectCtx<'w, 'ast, 'arena> {
    /// (call-site offset, entries passed at that site).
    sites: &'w mut Vec<(u32, Vec<SiteEntry<'ast, 'arena>>)>,
}

/// Walker that finds `view('name', …)` / `View::make('name', …)` calls
/// whose first-argument string contents sit at one of the requested
/// offsets, and collects the data entries they pass: the second-argument
/// array literal / `compact()` call, plus any `->with(…)` chained onto
/// the call.
struct ViewCallWalker<'a> {
    offsets: &'a [u32],
}

impl<'a> ViewCallWalker<'a> {
    fn matches(&self, argument_list: &ArgumentList<'_>) -> bool {
        let Some(first) = argument_list.arguments.iter().next() else {
            return false;
        };
        let Expression::Literal(Literal::String(s)) = first.value() else {
            return false;
        };
        let inner_start = s.span.start.offset + 1;
        self.offsets.contains(&inner_start)
    }
}

impl<'ast, 'arena, 'w> mago_syntax::walker::Walker<'ast, 'arena, CollectCtx<'w, 'ast, 'arena>>
    for ViewCallWalker<'_>
{
    fn walk_in_function_call(
        &self,
        node: &'ast FunctionCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let Expression::Identifier(ident) = node.function else {
            return;
        };
        let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
        if !name.eq_ignore_ascii_case("view") || !self.matches(&node.argument_list) {
            return;
        }
        let mut entries = Vec::new();
        collect_data_argument(&node.argument_list, 1, &mut entries);
        ctx.sites.push((node.span().start.offset, entries));
    }

    fn walk_in_static_method_call(
        &self,
        node: &'ast StaticMethodCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        if !is_view_facade_class(node.class)
            || !bytes_to_str(method.value).eq_ignore_ascii_case("make")
            || !self.matches(&node.argument_list)
        {
            return;
        }
        let mut entries = Vec::new();
        collect_data_argument(&node.argument_list, 1, &mut entries);
        ctx.sites.push((node.span().start.offset, entries));
    }

    fn walk_in_method_call(
        &self,
        node: &'ast MethodCall<'arena>,
        ctx: &mut CollectCtx<'w, 'ast, 'arena>,
    ) {
        // `->with('key', $value)` / `->with(['key' => $value])` chained
        // onto a matching `view()` call.  The receiver chain may pass
        // through other builder methods (`->layout(…)`), so scan the
        // whole spine for the matching view call.
        let ClassLikeMemberSelector::Identifier(method) = &node.method else {
            return;
        };
        if !bytes_to_str(method.value).eq_ignore_ascii_case("with") {
            return;
        }
        if !receiver_chain_has_matching_view_call(node.object, self) {
            return;
        }

        let mut entries = Vec::new();
        let mut args = node.argument_list.arguments.iter();
        match (args.next(), args.next()) {
            (Some(key_arg), Some(value_arg)) => {
                // ->with('key', $value)
                if let Expression::Literal(Literal::String(s)) = key_arg.value()
                    && let Some(name) = string_literal_contents(s)
                {
                    entries.push(SiteEntry::Expr(name, value_arg.value()));
                }
            }
            (Some(single), None) => {
                // ->with(['key' => $value, …]) or ->with(compact('key'))
                collect_from_data_expr(single.value(), &mut entries);
            }
            _ => {}
        }
        if !entries.is_empty() {
            ctx.sites.push((node.span().start.offset, entries));
        }
    }
}

/// Whether a static call's class expression names the `View` facade.
fn is_view_facade_class(class: &Expression<'_>) -> bool {
    let Expression::Identifier(ident) = class else {
        return false;
    };
    let subject = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
    subject.eq_ignore_ascii_case("View")
        || subject.eq_ignore_ascii_case("Illuminate\\Support\\Facades\\View")
}

/// Whether the receiver spine of a method call contains a `view()` /
/// `View::make()` call whose first argument matches the requested
/// offsets.  Walks through chained method calls (`view(…)->with(…)
/// ->with(…)`) but not through variables — a `$view = view(…);
/// $view->with(…)` split is out of scope for inference.
fn receiver_chain_has_matching_view_call(
    mut expr: &Expression<'_>,
    walker: &ViewCallWalker<'_>,
) -> bool {
    loop {
        match expr {
            Expression::Call(Call::Function(fc)) => {
                let Expression::Identifier(ident) = fc.function else {
                    return false;
                };
                let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
                return name.eq_ignore_ascii_case("view") && walker.matches(&fc.argument_list);
            }
            Expression::Call(Call::StaticMethod(sc)) => {
                let ClassLikeMemberSelector::Identifier(method) = &sc.method else {
                    return false;
                };
                return is_view_facade_class(sc.class)
                    && bytes_to_str(method.value).eq_ignore_ascii_case("make")
                    && walker.matches(&sc.argument_list);
            }
            Expression::Call(Call::Method(mc)) => {
                expr = mc.object;
            }
            Expression::Parenthesized(p) => {
                expr = p.expression;
            }
            _ => return false,
        }
    }
}

/// Collect variable entries from the data argument at `index` of a
/// `view()` / `View::make()` argument list.
fn collect_data_argument<'ast, 'arena>(
    argument_list: &'ast ArgumentList<'arena>,
    index: usize,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) {
    if let Some(arg) = argument_list.arguments.iter().nth(index) {
        collect_from_data_expr(arg.value(), entries);
    }
}

/// Collect entries from a data expression: an array literal with
/// string keys, or a `compact('a', 'b')` call (whose values are the
/// same-named variables at the call site).
fn collect_from_data_expr<'ast, 'arena>(
    expr: &'ast Expression<'arena>,
    entries: &mut Vec<SiteEntry<'ast, 'arena>>,
) {
    let mut collect_array_elements =
        |elements: &'ast TokenSeparatedSequence<'arena, ArrayElement<'arena>>| {
            for element in elements.iter() {
                if let ArrayElement::KeyValue(kv) = element
                    && let Expression::Literal(Literal::String(s)) = kv.key
                    && let Some(name) = string_literal_contents(s)
                {
                    entries.push(SiteEntry::Expr(name, kv.value));
                }
            }
        };
    match expr {
        Expression::Array(array) => collect_array_elements(&array.elements),
        Expression::LegacyArray(array) => collect_array_elements(&array.elements),
        Expression::Call(Call::Function(fc)) => {
            let Expression::Identifier(ident) = fc.function else {
                return;
            };
            let name = crate::util::strip_fqn_prefix(bytes_to_str(ident.value()));
            if !name.eq_ignore_ascii_case("compact") {
                return;
            }
            for arg in fc.argument_list.arguments.iter() {
                if let Expression::Literal(Literal::String(s)) = arg.value()
                    && let Some(name) = string_literal_contents(s)
                {
                    entries.push(SiteEntry::Variable(name));
                }
            }
        }
        _ => {}
    }
}

/// Whether the template declares any `@var` annotation inside a real
/// PHP docblock (`/** … */`).  A `@var` mentioned in a Blade comment
/// (`{{-- … --}}`) or in markup is not a declaration and must not
/// suppress call-site inference.
pub(crate) fn has_var_docblock(blade_content: &str) -> bool {
    let mut rest = blade_content;
    while let Some(start) = rest.find("/**") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("*/") else {
            return after.contains("@var");
        };
        if after[..end].contains("@var") {
            return true;
        }
        rest = &after[end + 2..];
    }
    false
}

/// The contents of a single- or double-quoted string literal, when it
/// is a plain identifier-safe name.
pub(crate) fn string_literal_contents(s: &LiteralString<'_>) -> Option<String> {
    let value = s.value.map(bytes_to_str)?;
    if value.is_empty()
        || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        || value.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(value.to_string())
}
