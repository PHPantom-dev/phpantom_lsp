//! Shared helpers for diagnostic collectors.
//!
//! Functions and types that are used by multiple diagnostic modules live
//! here to avoid duplication.

use std::sync::Arc;

use tower_lsp::lsp_types::*;

use crate::Backend;
use crate::symbol_map::SymbolMap;
use crate::types::{ClassInfo, FileContext};

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
/// Built by [`collect_type_check`], which owns the `FileContext` and the
/// loader closures this borrows.
pub(crate) struct TypeCheckCtx<'a> {
    /// Classes, use-map, namespace, and resolved names for the file.
    pub(crate) file_ctx: &'a FileContext,
    pub(crate) class_loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    pub(crate) function_loader: &'a dyn Fn(&str, u32) -> Option<crate::types::FunctionInfo>,
    pub(crate) constant_loader: &'a dyn Fn(&str, u32) -> Option<Option<String>>,
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
    uri: &str,
    content: &str,
    code: &str,
    out: &mut Vec<Diagnostic>,
    collect: impl FnOnce(&TypeCheckCtx<'_>) -> Vec<S>,
    report: impl Fn(&S, &TypeCheckCtx<'_>) -> Option<(ByteRange, String)>,
) {
    let file_ctx = backend.file_context(uri);

    // Activate the thread-local parse cache so every `with_parsed_program`
    // in the resolution pipeline below reuses one parsed AST.
    let _parse_guard = crate::parser::with_parse_cache(content);

    let class_loader = backend.class_loader(&file_ctx);
    let function_loader = backend.function_loader(&file_ctx);
    let constant_loader = backend.constant_loader(&file_ctx);
    let strict_types = crate::parser::with_parsed_program(content, "strict_types", |program, _| {
        super::type_errors::has_strict_types(program)
    });

    let ctx = TypeCheckCtx {
        file_ctx: &file_ctx,
        class_loader: &class_loader,
        function_loader: &function_loader,
        constant_loader: &constant_loader,
        strict_types,
    };

    for site in collect(&ctx) {
        let Some(((start, end), message)) = report(&site, &ctx) else {
            continue;
        };
        let Some(range) = backend.offset_range_to_lsp_range(uri, content, start, end) else {
            continue;
        };
        out.push(make_diagnostic(
            range,
            DiagnosticSeverity::ERROR,
            code,
            message,
        ));
    }
}

/// Per-file snapshot shared by the "symbol-span" diagnostic collectors
/// (unknown class/function/member, deprecated, implementation errors,
/// invalid class kind, unused imports).
///
/// Bundles the file's precomputed [`SymbolMap`] with the same
/// classes/use-map/namespace/resolved-names snapshot as [`FileContext`],
/// so each collector reads its per-file locks once via [`Self::gather`]
/// instead of re-acquiring `symbol_maps`, `uri_classes_index`,
/// `file_imports`, `file_namespaces`, and `resolved_names` independently.
pub(crate) struct FileDiagnosticContext {
    /// The file's precomputed symbol spans.
    pub(crate) symbol_map: Arc<SymbolMap>,
    /// Classes, use-map, namespace, and resolved-names for the file.
    pub(crate) file: FileContext,
}

impl FileDiagnosticContext {
    /// Gather the shared per-file snapshot for `uri`.
    ///
    /// Returns `None` when the file has no symbol map — the early-out
    /// every collector already applies (nothing to walk).
    pub(crate) fn gather(backend: &Backend, uri: &str) -> Option<Self> {
        let symbol_map = backend.symbol_map_for(uri)?;
        Some(Self {
            symbol_map,
            file: backend.file_context(uri),
        })
    }

    /// The file's own `ClassInfo` for a [`SymbolKind::ClassDeclaration`]
    /// name.
    ///
    /// The symbol map spells the name either way round depending on how
    /// the declaration was written, so a short name is also matched
    /// against the file namespace's qualification of it.
    pub(crate) fn declared_class(&self, name: &str) -> Option<&Arc<ClassInfo>> {
        self.file.classes.iter().find(|c| {
            c.name == name
                || self
                    .file
                    .namespace
                    .as_ref()
                    .is_some_and(|ns| format!("{}\\{}", ns, c.name) == name)
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
pub(crate) use crate::util::resolve_to_fqn;

/// Find the innermost class whose declaration span contains `offset`.
///
/// Returns a reference to the `ClassInfo` with the smallest span that
/// encloses `offset`, including anonymous classes.  Used for
/// `$this`/`self`/`static` resolution inside diagnostic collectors.
///
/// The span runs from the declaration start (`decl_start_offset`, which
/// includes any leading attribute lists) to the closing brace.  Using
/// the declaration start rather than the body's opening brace lets
/// `self::CONST` references inside class-level attributes — which sit
/// before the `class` keyword — resolve to their enclosing class.
pub(crate) fn find_innermost_enclosing_class(
    local_classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<&ClassInfo> {
    local_classes
        .iter()
        .map(|c| {
            // A value of 0 means "not available"; fall back to the body
            // start so synthetic classes keep their original span.
            let start = if c.decl_start_offset != 0 {
                c.decl_start_offset
            } else {
                c.start_offset
            };
            (c, start)
        })
        .filter(|(c, start)| offset >= *start && offset <= c.end_offset)
        .min_by_key(|(c, start)| c.end_offset.saturating_sub(*start))
        .map(|(c, _)| c.as_ref())
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
            Statement::Class(class) => find_method_name_in_members(class.members.iter(), offset),
            Statement::Trait(tr) => find_method_name_in_members(tr.members.iter(), offset),
            Statement::Enum(en) => find_method_name_in_members(en.members.iter(), offset),
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
/// Plain function calls and calls through a literal class name
/// (`Fqn::method`, `new Fqn`) resolve identically regardless of where
/// in the file they appear, so those remain safe to cache by text.
pub(crate) fn is_position_independent_call_expression(expr: &str) -> bool {
    if expr.starts_with('$') {
        return false;
    }
    if expr.starts_with("self::") || expr.starts_with("static::") || expr.starts_with("parent::") {
        return false;
    }
    !matches!(expr, "new self" | "new static" | "new parent")
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
