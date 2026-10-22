//! Shared helpers for diagnostic collectors.
//!
//! Functions and types that are used by multiple diagnostic modules live
//! here to avoid duplication.

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::class_lookup::ResolvedClassLoader;
use crate::php_type::PhpType;
use crate::resolution::FileLoaders;
use crate::symbol_map::SymbolMap;
use crate::types::{ClassInfo, FileContext, MethodInfo};

/// A byte range `[start, end)` in the source.
pub(crate) type ByteRange = (usize, usize);

/// PHP superglobals and auto-defined variables that are always in scope,
/// so they are neither reported as undefined nor as unused.
pub(crate) const SUPERGLOBALS: &[&str] = &[
    "$_GET",
    "$_POST",
    "$_SERVER",
    "$_REQUEST",
    "$_SESSION",
    "$_COOKIE",
    "$_FILES",
    "$_ENV",
    "$GLOBALS",
    "$argc",
    "$argv",
    "$http_response_header",
    "$php_errormsg",
];

// ── Type-checking collectors ────────────────────────────────────────────────

/// What a type-checking diagnostic collector reads from the file before
/// it walks the AST.
///
/// Built by [`collect_type_check`], which owns the loaders this borrows.
pub(crate) struct TypeCheckCtx<'a> {
    /// Classes, use-map, namespace, and resolved names for the file.
    pub(crate) file_ctx: &'a FileContext,
    /// The file's loaders, one set per `namespace` block.
    pub(crate) loaders: &'a FileLoaders<'a>,
    /// Whether the file declares `strict_types=1`, which decides how
    /// forgiving the compatibility checks are.
    pub(crate) strict_types: bool,
}

/// Run a type-checking collector over one file.
///
/// `collect` walks the AST and gathers the sites it checks; `report`
/// turns each one into a byte range and a message, or `None` for a site
/// that turns out to be fine. Both see the same [`TypeCheckCtx`], so the
/// loaders the walk resolves through are the ones the check reads back.
///
/// A site whose range has no position in the file is dropped: a Blade
/// template is checked as the virtual PHP it lowers to, and a range in
/// the generated prologue belongs to no line of the template.
pub(crate) fn collect_type_check<S>(
    backend: &Backend,
    file_ctx: &FileContext,
    uri: &str,
    content: &str,
    code: &str,
    collect: impl FnOnce(&TypeCheckCtx<'_>) -> Vec<S>,
    report: impl Fn(&S, &TypeCheckCtx<'_>) -> Option<(ByteRange, String)>,
) -> Vec<Diagnostic> {
    // Activate the thread-local parse cache so every `with_parsed_program`
    // in the resolution pipeline below reuses one parsed AST.
    let _parse_guard = crate::parser::with_parse_cache(content);

    let loaders = backend.file_loaders(file_ctx);
    let strict_types = crate::parser::with_parsed_program(content, "strict_types", |program, _| {
        super::type_errors::has_strict_types(program)
    });

    let ctx = TypeCheckCtx {
        file_ctx,
        loaders: &loaders,
        strict_types,
    };

    collect(&ctx)
        .iter()
        .filter_map(|site| {
            let ((start, end), message) = report(site, &ctx)?;
            let range = backend.offset_range_to_lsp_range(uri, content, start, end)?;
            Some(make_diagnostic(
                range,
                DiagnosticSeverity::ERROR,
                code,
                message,
            ))
        })
        .collect()
}

/// Per-file snapshot shared by every slow diagnostic collector.
///
/// Bundles the file's precomputed [`SymbolMap`] with the same
/// classes/use-map/namespace/resolved-names snapshot as [`FileContext`],
/// so a diagnostic pass reads its per-file locks once via [`Self::gather`]
/// instead of each collector re-acquiring the file's state and
/// `uri_classes_index` itself, and every collector observes the same snapshot
/// even if a concurrent parse updates those maps mid-pass.
pub(crate) struct FileDiagnosticContext {
    /// The file's precomputed symbol spans.
    pub(crate) symbol_map: Arc<SymbolMap>,
    /// Classes, use-map, namespace, and resolved-names for the file.
    pub(crate) file: FileContext,
}

impl FileDiagnosticContext {
    /// Gather the shared per-file snapshot for `uri`.
    ///
    /// A file with no symbol map yet (its parse panicked) gets an empty
    /// one, so the collectors that walk spans find nothing to report.
    pub(crate) fn gather(backend: &Backend, uri: &str) -> Self {
        Self {
            symbol_map: backend.symbol_map_for(uri).unwrap_or_default(),
            file: backend.file_context(uri),
        }
    }

    /// The file's own `ClassInfo` for a [`SymbolKind::ClassDeclaration`]
    /// name.
    ///
    /// The symbol map spells the name either way round depending on how
    /// the declaration was written, so a short name is also matched
    /// against the namespace in effect at `offset`. A short name declared
    /// in several `namespace` blocks resolves to the one in `offset`'s block.
    pub(crate) fn declared_class(&self, name: &str, offset: u32) -> Option<&Arc<ClassInfo>> {
        let namespace = self.file.namespace_at(offset);
        let classes = &self.file.classes;
        classes
            .iter()
            .find(|c| c.name == name && c.file_namespace.as_deref() == namespace.as_deref())
            .or_else(|| {
                classes.iter().find(|c| {
                    c.name == name
                        || namespace
                            .as_ref()
                            .is_some_and(|ns| format!("{}\\{}", ns, c.name) == name)
                })
            })
    }
}

/// Whether `offset` falls inside any of `ranges`.
pub(crate) fn is_offset_in_ranges(offset: u32, ranges: &[ByteRange]) -> bool {
    let offset = offset as usize;
    ranges
        .iter()
        .any(|&(start, end)| offset >= start && offset < end)
}

// Re-export the canonical `resolve_to_fqn` from `crate::util` so that
// existing `use super::helpers::resolve_to_fqn` imports keep working.
pub(crate) use crate::names::resolve_to_fqn;

/// Map each template parameter to its bound, or `mixed` when it has none.
///
/// Template parameters are plain names in a `PhpType`, so left in place
/// they would be looked up as classes and a same-named class would stand
/// in for them (`@template T` beside a `trait T` elsewhere in the project).
/// Substituting the bound first keeps a type check comparing what the
/// template is known to be.
pub(crate) fn template_bounds(
    params: &[crate::atom::Atom],
    bounds: &crate::atom::AtomMap<PhpType>,
) -> HashMap<String, PhpType> {
    params
        .iter()
        .map(|param| {
            let bound = bounds.get(param).cloned();
            (param.to_string(), bound.unwrap_or_else(PhpType::mixed))
        })
        .collect()
}

/// The `@template` parameters in scope at `offset` inside `class`, mapped
/// to their bounds (see [`template_bounds`]): the class's own, and those of
/// the method `method` names, with the method's shadowing the class's.
///
/// An overriding method without a docblock of its own still declares the
/// templates of the method it overrides (`first($targetType)` implementing
/// an interface's `@template T of Node`), so when the class's own copy has
/// none they are read from the fully resolved class, where inheritance has
/// carried them over.
pub(crate) fn template_bounds_in_scope(
    class: &ClassInfo,
    method: Option<&MethodInfo>,
    class_loader: &ResolvedClassLoader<'_>,
    cache: &crate::virtual_members::ResolvedClassCache,
) -> HashMap<String, PhpType> {
    let mut bounds = template_bounds(&class.template_params, &class.template_param_bounds);
    let Some(method) = method else {
        return bounds;
    };
    if !method.template_params().is_empty() {
        bounds.extend(template_bounds(
            method.template_params(),
            method.template_param_bounds(),
        ));
    } else if class.parent_class.is_some() || !class.interfaces.is_empty() {
        let resolved =
            crate::virtual_members::resolve_class_fully_cached(class, class_loader, cache);
        if let Some(inherited) = resolved.get_method(&method.name) {
            bounds.extend(template_bounds(
                inherited.template_params(),
                inherited.template_param_bounds(),
            ));
        }
    }
    bounds
}

/// The method of `class` whose body holds `offset`, judged by name
/// offsets alone: methods are laid out one after another, so it is the
/// last one declared before `offset`.
pub(crate) fn method_declared_before(class: &ClassInfo, offset: u32) -> Option<&MethodInfo> {
    class
        .methods
        .iter()
        .filter(|m| {
            m.name_offset != 0 && m.name_offset >= class.start_offset && m.name_offset <= offset
        })
        .max_by_key(|m| m.name_offset)
        .map(|m| m.as_ref())
}

/// Find the name of the method whose body contains `offset`, if any.
///
/// Used by the `@deprecated` usage pass to tell whether a call site sits
/// inside a method that itself overrides/implements the deprecated
/// member being referenced there — PHPStan's own deprecation rule
/// (`DefaultDeprecatedScopeResolver` in phpstan/phpstan-deprecation-rules)
/// exempts that pattern instead of flagging legacy code for calling
/// other legacy code.
///
/// Offset containment is checked against the method body's braces only,
/// so a call inside a nested closure or arrow function is still
/// attributed to the enclosing method — matching PHPStan, which resolves
/// `Scope::getFunction()` to the same enclosing method from inside an
/// arrow function body.
pub(crate) fn find_enclosing_method_name(content: &str, offset: u32) -> Option<String> {
    crate::parser::with_parsed_program(content, "find_enclosing_method_name", |program, _| {
        find_enclosing_method_name_in_statements(&program.statements, offset)
    })
}

fn find_enclosing_method_name_in_statements<'a>(
    statements: &mago_syntax::cst::Sequence<'a, mago_syntax::cst::Statement<'a>>,
    offset: u32,
) -> Option<String> {
    use mago_syntax::cst::Statement;

    for stmt in statements.iter() {
        let found = match stmt {
            _ if let Some(body) = crate::parser::class_like_body(stmt) => {
                find_method_name_in_members(body.members.iter(), offset)
            }
            Statement::Namespace(ns) => {
                return find_enclosing_method_name_in_statements(ns.statements(), offset);
            }
            _ => None,
        };
        if let Some(name) = found {
            return Some(name.to_string());
        }
    }
    None
}

fn find_method_name_in_members<'a>(
    members: impl Iterator<Item = &'a mago_syntax::cst::class_like::member::ClassLikeMember<'a>>,
    offset: u32,
) -> Option<&'a str> {
    use mago_syntax::cst::class_like::member::ClassLikeMember;
    use mago_syntax::cst::class_like::method::MethodBody;

    for member in members {
        if let ClassLikeMember::Method(method) = member
            && let MethodBody::Concrete(block) = &method.body
        {
            let body_start = block.left_brace.start.offset;
            let body_end = block.right_brace.end.offset;
            if offset >= body_start && offset <= body_end {
                return Some(crate::atom::bytes_to_str(method.name.value));
            }
        }
    }
    None
}

/// Returns `true` when a call expression's `resolve_callable_target*`
/// result is guaranteed to be the same at every call site in a file, so
/// it is safe to memoize by expression text alone in a per-file cache.
///
/// Excludes:
/// - Variable-based calls (`$subject->method`) — the receiver
///   variable's type comes from the assignments visible at the cursor
///   and can differ between call sites that share the same text (e.g.
///   two methods that each assign a different type to `$parser`).
/// - `self::`, `static::`, `parent::`, `new self`, `new static`, and
///   `new parent` — resolved via the enclosing class at the cursor
///   offset (`find_class_at_offset`), which differs between call sites
///   in different classes within the same file.
///
/// The same holds for an invoked value (`($handler)`, `(self::$handler)`),
/// judged by the expression inside its parentheses.
///
/// Plain function calls and calls through a literal class name
/// (`Fqn::method`, `new Fqn`) resolve identically regardless of where
/// in the file they appear, so those remain safe to cache by text.
pub(crate) fn is_position_independent_call_expression(expr: &str) -> bool {
    // An invoked value (`($this->handler)`) depends on its inner expression.
    let expr = expr.trim_start_matches('(');
    if expr.starts_with('$') {
        return false;
    }
    if expr.starts_with("self::") || expr.starts_with("static::") || expr.starts_with("parent::") {
        return false;
    }
    !matches!(expr, "new self" | "new static" | "new parent")
}

/// The callable each call expression in a file resolves to, memoized by
/// the expression's text for the expressions
/// [`is_position_independent_call_expression`] accepts.
///
/// There is one cache per `namespace` block, since the same text can name a
/// different target under another block's imports.
pub(crate) struct CallTargetCache<'a> {
    blocks: crate::types::PerBlock<
        'a,
        std::cell::RefCell<HashMap<String, Option<crate::types::ResolvedCallableTarget>>>,
    >,
}

impl<'a> CallTargetCache<'a> {
    pub(crate) fn new(file_ctx: &'a FileContext) -> Self {
        Self {
            blocks: file_ctx.per_block(|_, _| std::cell::RefCell::new(HashMap::new())),
        }
    }

    /// The callable `expr` at `offset` resolves to, resolved once per
    /// block.
    pub(crate) fn target(
        &self,
        backend: &Backend,
        expr: &str,
        content: &str,
        offset: u32,
        file_ctx: &FileContext,
    ) -> Option<crate::types::ResolvedCallableTarget> {
        let cache = self.blocks.at(offset);
        if let Some(cached) = cache.borrow().get(expr) {
            return cached.clone();
        }
        let resolved = backend.resolve_callable_target_at_offset(expr, content, offset, file_ctx);
        cache
            .borrow_mut()
            .insert(expr.to_string(), resolved.clone());
        resolved
    }
}

/// Build a standard diagnostic with the common fields pre-filled.
///
/// Most diagnostic collectors build `Diagnostic` values with `source`
/// set to `"phpantom"` and the remaining optional fields set to `None`.
/// This helper reduces the boilerplate.
pub(crate) fn make_diagnostic(
    range: Range,
    severity: DiagnosticSeverity,
    code: &str,
    message: String,
) -> Diagnostic {
    make_tagged_diagnostic(range, severity, code, message, None)
}

/// [`make_diagnostic`], carrying a `DiagnosticTag`.
///
/// The tag is what makes an editor render the range as dimmed
/// ([`DiagnosticTag::UNNECESSARY`]) or struck through
/// ([`DiagnosticTag::DEPRECATED`]) rather than underlining it.
pub(crate) fn make_tagged_diagnostic(
    range: Range,
    severity: DiagnosticSeverity,
    code: &str,
    message: String,
    tag: Option<DiagnosticTag>,
) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(severity),
        code: Some(NumberOrString::String(code.to_string())),
        code_description: None,
        source: Some("phpantom".to_string()),
        message,
        related_information: None,
        tags: tag.map(|t| vec![t]),
        data: None,
    }
}
