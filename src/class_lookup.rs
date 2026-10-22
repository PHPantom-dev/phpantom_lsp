//! Class lookup by name/offset and subtype-relationship checks.
//!
//! These helpers answer "which class is this?", through [`subtyping`]
//! "is this class a subtype of that one?", and through [`compatibility`]
//! "can a value of this type be passed where that one is declared?" against an
//! already-loaded slice of [`ClassInfo`] values (or via a class loader
//! closure for cross-file lookups). Cross-file/PSR-4/stub resolution
//! itself lives in [`crate::resolution`]; this module is the shared
//! kernel that resolution and the completion/diagnostics pipelines both
//! call into.
//!
//! # Which class loader to use
//!
//! A class name is either *written*, as it appears in a file (`new Foo`,
//! `Foo::bar()`, `@var Foo`), or *resolved*, a fully-qualified name the
//! engine already worked out (a stored `parent_class`, a type hint, a type
//! string the type engine carries around).  The two read differently: a
//! written `Redis` goes through the file's imports and namespace, a
//! resolved `Redis` is the global class.  Each kind has its own loader,
//! and the types keep them apart:
//!
//! - **Resolved names** go through a [`ResolvedClassLoader`], which only
//!   accepts an [`Fqn`].  Build one with
//!   [`Backend::resolved_class_loader`](crate::Backend::resolved_class_loader)
//!   (or `_with` for a re-parsed class list); inside the type engine it is
//!   [`FileCtx::class_loader`](crate::type_engine::resolver::FileCtx::class_loader).
//!   [`find_or_load_resolved`] checks the file's own classes first, and
//!   [`load_ancestor`] loads a stored parent, interface or trait.
//! - **Written names** go through a [`SourceClassLoader`], which takes the
//!   name as spelled (`&str`) and applies the imports and namespace of one
//!   `namespace` block.  Build one with
//!   [`Backend::class_loader`](crate::Backend::class_loader) for a cursor,
//!   [`Backend::class_loaders`](crate::Backend::class_loaders) for a whole
//!   file, or [`FileLoaders::class_loader_at`](crate::resolution::FileLoaders::class_loader_at);
//!   inside the type engine it is
//!   [`FileCtx::source_class_loader`](crate::type_engine::resolver::FileCtx::source_class_loader).
//!   To turn a written name into an FQN without loading it, use
//!   [`resolve_code_class_name`](crate::names::resolve_code_class_name).
//!
//! Wrapping a written name in [`Fqn::new`] is the one way to get this
//! wrong, so build an `Fqn` only from a name that is already resolved.

mod compatibility;
mod subtyping;

use std::sync::Arc;

use crate::types::ClassInfo;

/// A fully-qualified class name, without a leading `\`.
///
/// The only argument a [`ResolvedClassLoader`] takes, so a name has to be
/// declared resolved before it can be looked up as one.  [`Fqn::new`]
/// strips a leading `\`, so callers never do it by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fqn<'a>(&'a str);

impl<'a> Fqn<'a> {
    /// The already-resolved `name`, with or without a leading `\`.
    #[inline]
    pub fn new(name: &'a str) -> Self {
        Self(name.strip_prefix('\\').unwrap_or(name))
    }

    /// The name, without a leading `\`.
    #[inline]
    pub fn as_str(self) -> &'a str {
        self.0
    }
}

impl std::ops::Deref for Fqn<'_> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        self.0
    }
}

impl std::fmt::Display for Fqn<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// A class loader for already-resolved names; see the [module docs](self).
pub type ResolvedClassLoader<'a> = dyn Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> + 'a;

/// A class loader for names as written in one `namespace` block of a file;
/// see the [module docs](self).
pub type SourceClassLoader<'a> = dyn Fn(&str) -> Option<Arc<ClassInfo>> + 'a;

pub(crate) use compatibility::{
    first_rejected_callable_param, is_type_compatible, missing_required_shape_keys,
    shape_breaks_list_order,
};
pub(crate) use subtyping::{
    evaluate_constant_shape_key, is_subclass_of, is_subtype_of, is_subtype_of_named,
    is_subtype_of_names, is_subtype_of_typed,
};

/// Find which class the cursor (byte offset) is inside.
///
/// When multiple classes contain the offset (e.g. an anonymous class
/// nested inside a named class's method), the smallest (most specific)
/// class is returned.  This ensures that `$this` inside an anonymous
/// class body resolves to the anonymous class, not the outer class.
///
/// The span runs from the declaration start (`decl_start_offset`, which
/// includes any leading attribute lists) to the closing brace, so a
/// `self::` reference inside a class-level attribute — which sits before
/// the body braces — still resolves to the class it decorates.
pub(crate) fn find_class_at_offset(classes: &[Arc<ClassInfo>], offset: u32) -> Option<&ClassInfo> {
    find_shared_class_at_offset(classes, offset).map(Arc::as_ref)
}

/// [`find_class_at_offset`], handing back the shared `Arc` so a caller
/// can keep the class after letting go of the list it came from.
pub(crate) fn find_shared_class_at_offset(
    classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<&Arc<ClassInfo>> {
    classes
        .iter()
        .map(|c| {
            let start = if c.decl_start_offset != 0 {
                c.decl_start_offset
            } else {
                c.start_offset
            };
            (c, start)
        })
        .filter(|(c, start)| offset >= *start && offset <= c.end_offset)
        .min_by_key(|(c, start)| c.end_offset.saturating_sub(*start))
        .map(|(c, _)| c)
}

/// Find which class a docblock at `offset` documents.
///
/// A docblock on a *member* sits inside the class body, which
/// [`find_class_at_offset`] already resolves.  A docblock on the *class*
/// is trivia preceding the declaration, which the parser recorded as
/// [`ClassInfo::class_docblock`].
pub(crate) fn find_documented_class_at_offset(
    classes: &[Arc<ClassInfo>],
    offset: u32,
) -> Option<&ClassInfo> {
    if let Some(class) = find_class_at_offset(classes, offset) {
        return Some(class);
    }
    classes.iter().map(|c| c.as_ref()).find(|c| {
        c.class_docblock.as_deref().is_some_and(|docblock| {
            let start = c.class_docblock_start;
            start <= offset && (offset as usize) < start as usize + docblock.len()
        })
    })
}

/// Build the stand-in [`ClassInfo`] used when the cursor is not inside a
/// class body — a plain function, a closure at file scope, or top-level
/// code.  It has no name, so the checks that ask "are we in a class?"
/// (`current_class.name.is_empty()`) still answer no.
///
/// What it does carry is the namespace enclosing the cursor.  Source-level
/// class references resolve through
/// [`resolve_code_class_name`](crate::names::resolve_code_class_name),
/// which reads that namespace from `current_class.file_namespace` so a
/// same-namespace class outranks a global one of the same short name.  A
/// bare `ClassInfo::default()` reports no namespace, which would resolve
/// `Foo::bar()` inside `namespace App` to the global `\Foo` even when
/// `App\Foo` exists.
pub(crate) fn class_context_placeholder(content: &str, cursor_offset: u32) -> ClassInfo {
    placeholder_in_namespace(crate::parser::namespace_at_offset(
        content,
        cursor_offset as usize,
    ))
}

/// [`class_context_placeholder`] for callers that already know the
/// enclosing namespace (e.g. a walker recursing into a `namespace { … }`
/// block) and need not scan for it.
pub(crate) fn placeholder_in_namespace(namespace: Option<&str>) -> ClassInfo {
    ClassInfo {
        file_namespace: namespace.map(crate::atom::atom),
        ..ClassInfo::default()
    }
}

/// Find a class in a slice by name, preferring namespace-aware matching
/// when the name is fully qualified.
///
/// When `name` contains backslashes (e.g. `Illuminate\Database\Eloquent\Builder`),
/// the lookup checks each candidate's `file_namespace` field so that the
/// correct class is returned even when multiple classes share the same short
/// name but live in different namespace blocks within the same file (e.g.
/// `Demo\Builder` vs `Illuminate\Database\Eloquent\Builder`).
///
/// When `name` is a bare short name (no backslashes), the first class with
/// a matching `name` field is returned (preserving existing behavior).
pub(crate) fn find_class_by_name<'a>(
    all_classes: &'a [Arc<ClassInfo>],
    name: &str,
) -> Option<&'a Arc<ClassInfo>> {
    let short = crate::util::short_name(name);

    if name.contains('\\') {
        let expected_ns = name.rsplit_once('\\').map(|(ns, _)| ns);
        all_classes
            .iter()
            .find(|c| c.name == short && c.file_namespace.as_deref() == expected_ns)
    } else {
        all_classes.iter().find(|c| c.name == short)
    }
}

/// Find the class in a slice whose fully-qualified name is `fqn`.
///
/// Unlike [`find_class_by_name`], a name without a namespace only matches a
/// global class, so a resolved name never lands on a same-named class of
/// another namespace block.
pub(crate) fn find_class_by_fqn<'a>(
    all_classes: &'a [Arc<ClassInfo>],
    fqn: &str,
) -> Option<&'a Arc<ClassInfo>> {
    let fqn = fqn.strip_prefix('\\').unwrap_or(fqn);
    let (namespace, short) = match fqn.rsplit_once('\\') {
        Some((namespace, short)) => (Some(namespace), short),
        None => (None, fqn),
    };
    all_classes.iter().find(|c| {
        // An anonymous class's FQN carries the URI it was parsed from,
        // which the split above does not take apart.
        if c.name.starts_with(crate::types::ANONYMOUS_CLASS_PREFIX) {
            return c.fqn().eq_ignore_ascii_case(fqn);
        }
        c.name.eq_ignore_ascii_case(short)
            && match (c.file_namespace.as_deref(), namespace) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (None, None) => true,
                _ => false,
            }
    })
}

/// The class a class-position expression names, as written.
///
/// Covers the four spellings a `Foo::bar()`, `new Foo`, `Foo::CONST`, or
/// `Foo::$prop` can put on the left: a name, `self`, `static`, and
/// `parent`. `self` and `static` answer with the current class (late
/// static binding is settled by the caller, which knows whether the call
/// forwards it), and `parent` with its parent, or `None` when it has
/// none.
///
/// A name is resolved the way PHP resolves it in code (see
/// [`crate::names::resolve_code_class_name`]): through `imports`, else in
/// the current namespace, never falling back to a global class when the
/// imports are known.
///
/// Every other expression kind (a variable holding a class-string, a call)
/// needs resolution this cannot do and yields `None`; a caller that
/// handles one matches it before reaching here.
pub(crate) fn class_expression_name(
    expr: &mago_syntax::cst::Expression<'_>,
    current_class: &ClassInfo,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &SourceClassLoader<'_>,
    imports: crate::type_engine::resolver::SourceImports<'_>,
) -> Option<String> {
    use mago_syntax::cst::Expression;
    match expr {
        Expression::Identifier(ident) => Some(crate::names::resolve_code_class_name(
            crate::atom::bytes_to_str(ident.value()),
            current_class.file_namespace.as_deref(),
            imports,
            local_classes,
            class_loader,
        )),
        Expression::Self_(_) | Expression::Static(_) => Some(current_class.fqn().to_string()),
        Expression::Parent(_) => current_class.parent_class.map(|a| a.to_string()),
        _ => None,
    }
}

/// Returns `true` if `s` is one of the PHP keywords that refer to the
/// *current* class (not the parent): `self`, `static`, or `$this`.
///
/// Callers that also need to match `parent` should add a separate
/// `eq_ignore_ascii_case("parent")` check, because `parent` resolves
/// to the *parent* class rather than the current one.
///
/// The comparison is case-insensitive for `self` and `static`.
/// `$this` is matched literally (it is always lowercase in PHP).
pub(crate) fn is_self_or_static(s: &str) -> bool {
    s.eq_ignore_ascii_case("self") || s.eq_ignore_ascii_case("static") || s == "$this"
}

/// Returns `true` if `s` is one of the PHP class-keyword references:
/// `self`, `static`, `$this`, or `parent`.
///
/// Use this when you need a single guard that covers *all* class
/// keywords, including `parent`.  For the subset that resolves to the
/// *current* class only, use [`is_self_or_static`].
pub(crate) fn is_class_keyword(s: &str) -> bool {
    is_self_or_static(s) || s.eq_ignore_ascii_case("parent")
}

/// Resolve `self`, `static`, `$this`, or `parent` to a class name.
///
/// Returns `Some(class_name)` when the keyword can be resolved, or
/// `None` when:
/// - `keyword` is not a recognised class keyword, or
/// - there is no `current_class`, or
/// - `parent` is used but the class has no parent.
///
/// This centralises the keyword → class-name mapping that was
/// previously duplicated across 10+ call sites.
///
/// The name returned is fully qualified.  `parent` already is, and `self`
/// has to be: a namespaced class whose short name collides with a global
/// one (`PHPStan\Analyser\Error` against the built-in `\Error`) would
/// otherwise have every `new self(…)` and `self::` resolve to whichever
/// class the loader finds first, which is the global one.
pub(crate) fn resolve_class_keyword(
    keyword: &str,
    current_class: Option<&ClassInfo>,
) -> Option<String> {
    if is_self_or_static(keyword) {
        current_class.map(|cc| cc.fqn().to_string())
    } else if keyword.eq_ignore_ascii_case("parent") {
        current_class.and_then(|cc| cc.parent_class.map(|a| a.to_string()))
    } else {
        None
    }
}

/// Load a class's ancestor by the name stored on it, refusing an answer
/// that is the class itself.
///
/// `parent_class`, `interfaces`, and `used_traits` hold canonical FQNs, so
/// a backslash-free entry names a root-namespace class.  The name is looked
/// up fully qualified: a source-name loader would apply the *consuming*
/// file's imports and namespace to it, so resolving
/// `Nette\Neon\Exception extends \Exception` from a file that says
/// `use Nette\Neon\Exception;` would turn the stored `Exception` back into
/// the subclass, and from any other namespace would miss it entirely.
///
/// PHP has no self-inheritance, so a name that still resolves to the class
/// itself is a genuine cycle and yields nothing.
pub(crate) fn load_ancestor(
    class_fqn: &str,
    ancestor_name: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Arc<ClassInfo>> {
    class_loader(Fqn::new(ancestor_name)).filter(|cls| !cls.fqn().eq_ignore_ascii_case(class_fqn))
}

/// The class an already-resolved `fqn` names: one of `all_classes`, else
/// whatever `class_loader` loads for it.
pub(crate) fn find_or_load_resolved(
    fqn: &str,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Arc<ClassInfo>> {
    find_class_by_fqn(all_classes, fqn)
        .map(Arc::clone)
        .or_else(|| class_loader(Fqn::new(fqn)))
}

#[cfg(test)]
#[path = "class_lookup_tests.rs"]
mod tests;
