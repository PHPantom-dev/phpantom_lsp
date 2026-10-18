//! Reading the `@template` bounds and the `@param` and `@return` docblocks
//! of the declaration a body belongs to.

use std::sync::Arc;

use crate::docblock;
use crate::parser::preceding_docblock_text;
use crate::php_type::{PhpType, TypeKind, is_keyword_type};
use crate::type_engine::resolver::WrittenNames;
use crate::type_engine::variable::forward_walk::docblock_above;
use crate::types::ClassInfo;

/// Mark the method/function-level template parameters a type names with
/// the upper bounds their `@template T of Bound` annotations give them.
///
/// This handles the general case where a parameter type IS a template
/// parameter (e.g. `@param T $query` where `@template T of Builder`).
/// A bare `T` is an unresolvable named type, so member access on `$query`
/// would fail with "subject type 'T' could not be resolved".  Each bounded
/// `T` becomes a [`TemplateParam`](crate::php_type::TypeKind::TemplateParam)
/// instead, which is still displayed and substituted as `T` but resolves
/// as its bound, wherever the value travels.
///
/// Works on any `PhpType` structure — bare names, unions, intersections,
/// nullable wrappers, generics, etc. — via `PhpType::substitute`.
pub(in crate::type_engine::variable) fn substitute_template_param_bounds(
    ty: PhpType,
    content: &str,
    method_start_offset: usize,
    names: &WrittenNames<'_>,
) -> PhpType {
    // Quick check: only act when the type contains at least one bare
    // identifier that could be a template parameter.  This avoids the
    // docblock parse for the common case where the type is a concrete
    // class name or scalar.
    if !type_may_contain_template_param(&ty) {
        return ty;
    }

    let Some(docblock) = docblock_above(content, method_start_offset) else {
        return ty;
    };

    let bounds = docblock::extract_template_params_with_bounds(docblock);
    if bounds.is_empty() {
        return ty;
    }

    let subs = bounded_template_markers(bounds, names);
    if subs.is_empty() {
        return ty;
    }

    ty.substitute(&subs)
}

/// The markers [`bounded_template_markers`] builds for the declaration
/// starting at `decl_start`, or `None` when it declares no bounded template.
pub(in crate::type_engine) fn declaration_template_markers(
    content: &str,
    decl_start: usize,
    names: &WrittenNames<'_>,
) -> Option<std::sync::Arc<std::collections::HashMap<String, PhpType>>> {
    let docblock = docblock_above(content, decl_start)?;
    if !docblock.contains("template") {
        return None;
    }
    let markers = bounded_template_markers(
        docblock::extract_template_params_with_bounds(docblock),
        names,
    );
    (!markers.is_empty()).then(|| std::sync::Arc::new(markers))
}

/// The [`TemplateParam`](crate::php_type::TypeKind::TemplateParam) each
/// bounded template in `bounds` stands for, keyed by its name.
///
/// The bounds are read from the docblock as written, so each one goes
/// through [`WrittenNames::resolve_type`] before it stands for a class.
///
/// A bound naming another of the declaration's templates (`@template U of
/// T`) reads through to that template's own bound, so `U` still resolves
/// to a class.
fn bounded_template_markers(
    bounds: Vec<(String, Option<PhpType>)>,
    names: &WrittenNames<'_>,
) -> std::collections::HashMap<String, PhpType> {
    let mut subs: std::collections::HashMap<String, PhpType> = bounds
        .into_iter()
        .filter_map(|(name, bound)| {
            let bound = names.resolve_type(&bound?);
            let marker = PhpType::template_param(crate::atom::atom(&name), bound);
            Some((name, marker))
        })
        .collect();
    let chained: Vec<(String, PhpType)> = subs
        .iter()
        .filter_map(|(name, marker)| {
            let (tpl, bound) = marker.as_template_param()?;
            let substituted = bound.substitute(&subs);
            (substituted != *bound)
                .then(|| (name.clone(), PhpType::template_param(tpl, substituted)))
        })
        .collect();
    subs.extend(chained);
    subs
}

/// Whether `ty` names one of the `@template` parameters the declaration
/// starting at `method_start_offset` introduces.
pub(in crate::type_engine::variable) fn references_method_template(
    ty: &PhpType,
    content: &str,
    method_start_offset: usize,
) -> bool {
    if !type_may_contain_template_param(ty) {
        return false;
    }
    docblock_above(content, method_start_offset).is_some_and(|docblock| {
        ty.references_any_template_param(&docblock::extract_template_params(docblock))
    })
}

/// Whether `settled`, the type a parameter's docblock settled over its native
/// hint `written` (as written), may stand in for the hint: every class it
/// names that the hint does not narrows the hint.
///
/// PHP enforces the hint, and loading the declaration makes the same call
/// for its signature (see
/// [`docblock_narrowing`](crate::type_engine::types::docblock_narrowing)),
/// so the body sees the parameter the way its callers do.  The hint is
/// resolved through `source_class_loader`, the way the docblock's names
/// were, so that one spelling is one class on both sides.  `decl_start` is
/// where the declaration starts, for the templates its docblock introduces.
pub(in crate::type_engine::variable) fn param_docblock_narrows_hint(
    settled: &PhpType,
    written: &PhpType,
    decl_start: usize,
    current_class: &ClassInfo,
    content: &str,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    source_class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> bool {
    use crate::type_engine::types::docblock_narrowing;
    if !docblock_narrowing::hint_names_classes(written) {
        return true;
    }
    let native = crate::names::resolve_php_type_names(written, source_class_loader);
    let declared_templates = std::cell::OnceCell::new();
    let not_a_class = |name: &str| {
        current_class.template_params.iter().any(|t| t == name)
            || current_class.type_aliases.keys().any(|alias| alias == name)
            || declared_templates
                .get_or_init(|| {
                    docblock_above(content, decl_start)
                        .map(docblock::extract_template_params)
                        .unwrap_or_default()
                })
                .iter()
                .any(|t| t == name)
    };
    docblock_narrowing::narrows_native(settled, &native, &not_a_class, class_loader)
}

/// Check whether a `PhpType` tree may contain a bare template parameter
/// name — i.e. a `Named` variant whose value is not a well-known scalar
/// or pseudo-type.  This is a cheap pre-filter so that we only parse the
/// docblock when there is a realistic chance of finding a substitution.
fn type_may_contain_template_param(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => {
            // Well-known scalars/pseudo-types are never template params.
            !is_keyword_type(name)
        }
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(type_may_contain_template_param)
        }
        TypeKind::Nullable(inner) => type_may_contain_template_param(inner),
        TypeKind::Generic(g) => {
            !crate::php_type::is_keyword_type(&g.name)
                || g.args.iter().any(type_may_contain_template_param)
        }
        _ => false,
    }
}

/// Mark method-level template parameters inside `class-string<T>` types
/// with their upper bounds from `@template T of Bound` annotations.
///
/// This enables `$class::` static member access resolution when the
/// parameter is typed as `class-string<T>` and `T` is bounded by a
/// concrete class.  Without this substitution, `T` remains an
/// unresolvable named type and `$class::` yields no completions.
pub(in crate::type_engine::variable) fn substitute_class_string_template_bounds(
    ty: PhpType,
    content: &str,
    method_start_offset: usize,
    names: &WrittenNames<'_>,
) -> PhpType {
    // Only act on class-string<T> where the inner type is a simple name
    // (i.e. a potential template parameter).
    let inner_name = match &ty.kind() {
        TypeKind::ClassString(Some(inner)) => match inner.kind() {
            TypeKind::Named(name) => Some(*name),
            _ => None,
        },
        _ => None,
    };

    let Some(tpl_name) = inner_name else {
        return ty;
    };

    // The method's docblock gives the template parameter bounds.
    let Some(docblock) = docblock_above(content, method_start_offset) else {
        return ty;
    };

    let bounds = docblock::extract_template_params_with_bounds(docblock);
    match bounded_template_markers(bounds, names).remove(tpl_name.as_str()) {
        Some(marker) => PhpType::class_string(Some(marker)),
        None => ty,
    }
}

/// The `@param` type the declaration starting at `decl_start` gives
/// `pname`.
///
/// The declaration's own docblock is read first, through the tag parser,
/// so a vendor tag (`@phpstan-param`, `@psalm-param`) takes precedence
/// over a plain `@param` for the same parameter the way it does in the
/// signature.  A closure is also documented by the docblock above the
/// statement it is part of (`register(\n    'name',\n    function ($x) {…}`),
/// as long as no statement or block boundary separates the two.
pub(crate) fn declared_param_docblock_type(
    content: &str,
    decl_start: usize,
    pname: &str,
) -> Option<PhpType> {
    let param_in = |doc: &str| docblock::extract_param_raw_type(doc, pname);
    docblock_above(content, decl_start)
        .and_then(param_in)
        .or_else(|| preceding_docblock_text(content, decl_start).and_then(param_in))
}

/// The `@return` type the docblock of the declaration starting at
/// `decl_start` gives it, as written.
pub(in crate::type_engine) fn declared_return_docblock_type(
    content: &str,
    decl_start: usize,
) -> Option<PhpType> {
    docblock_above(content, decl_start).and_then(docblock::extract_return_type)
}
