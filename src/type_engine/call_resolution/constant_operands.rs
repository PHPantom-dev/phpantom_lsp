//! Reading the constants and enums a type operator names.
//!
//! `key-of<ID_TABLE>`, `value-of<Suit>` and `ID_TABLE[K]` describe a set of
//! values as concrete as a written-out union, but the docblock parser only
//! sees the operand's name.  This module reads the constant or enum behind
//! the name and finishes the operator, for declared types and for the
//! template bounds a call binds through.

use std::collections::HashMap;
use std::sync::Arc;

use crate::atom::Atom;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::ClassInfo;

use super::arg_text_types::array_literal_shape_type;

/// The literal an argument binds a template to when the template's own bound
/// is a type operator — `@template K of key-of<TABLE>`, `@template V of
/// value-of<TABLE>`, `@template E of TABLE[K]`.
///
/// `resolve_arg_text_to_type` widens a scalar literal to its base type, which
/// is what nearly every binding wants and exactly wrong here: the operator can
/// only be evaluated against the *specific* key or value the caller wrote, so
/// `'immutable'` has to stay `'immutable'` rather than becoming `string`. The
/// quotes are kept because that is the spelling `evaluate_index_access`
/// matches shape keys against.
///
/// Returns `None` when the bound is not a type operator or the argument is not
/// a scalar literal, leaving the ordinary binding modes to resolve it.
pub(crate) fn type_operator_bound_literal(
    bound: Option<&PhpType>,
    arg_text: &str,
) -> Option<PhpType> {
    let bound = bound?;
    if !matches!(
        bound.kind(),
        TypeKind::KeyOf(_) | TypeKind::ValueOf(_) | TypeKind::IndexAccess(..)
    ) {
        return None;
    }
    crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(arg_text.trim())
        .filter(|ty| matches!(ty.kind(), TypeKind::Literal(_)))
}

/// The array shape a constant read through a type operator describes.
///
/// `key-of<ID_TABLE>` and `ID_TABLE[K]` name an operand as concrete as an
/// inline `array{…}`, but the docblock parser only ever sees the name — it
/// cannot read the constant behind it. This does, from the constant's own
/// initializer text, for a global constant (`ID_TABLE`) and for the
/// `Class::CONST` spelling alike.
///
/// Returns `None` when the name is not a constant we can reach, when its
/// value is not an array literal, or when none of its keys are literal — in
/// each case the operator stays unevaluated and widens to its bound, which
/// is the honest reading of an operand nobody can read.
pub(crate) fn constant_operand_shape(name: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let value = match name.rsplit_once("::") {
        Some((class_part, const_name)) => {
            let class = load_operand_class(class_part, ctx)?;
            let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                &class,
                ctx.class_loader,
                ctx.resolved_class_cache,
            );
            merged.get_constant(const_name)?.value.clone()?
        }
        // The name may arrive in either spelling: some paths hand over what
        // `resolve_names` already qualified against the file's namespace,
        // since almost every bare name in a type position is a class, while
        // the ones that resolve names through the class loader alone leave a
        // constant bare.
        None => resolve_operand_constant_value(name, ctx)?,
    };
    array_literal_shape_type(&value, ctx)
}

/// The initializer text of the constant a type operand names, or `None`
/// when no spelling of it is indexed.
///
/// A constant is indexed under its fully-qualified name, so the enclosing
/// namespace is tried first for a bare operand, then the name as written,
/// then the global constant of that short name the way PHP itself falls
/// back, and finally the file's `use const` imports.
///
/// Only the indexed lookup is used, never the one that ends by parsing
/// every autoload file: an operand is as often a template parameter as a
/// constant, and a miss must not charge the whole autoload set.
fn resolve_operand_constant_value(name: &str, ctx: &ResolutionCtx<'_>) -> Option<String> {
    let backend = ctx.backend?;
    // An absolute name names exactly one constant, with no namespace or
    // import in play.
    if let Some(absolute) = name.strip_prefix('\\') {
        return backend.lookup_indexed_global_constant(absolute);
    }
    let short = crate::util::short_name(name);
    if short == name
        && let Some(value) = ctx
            .current_class
            .and_then(|c| c.file_namespace.as_ref())
            .and_then(|ns| backend.lookup_indexed_global_constant(&format!("{ns}\\{name}")))
    {
        return Some(value);
    }
    if let Some(value) = backend.lookup_indexed_global_constant(name) {
        return Some(value);
    }
    if short != name
        && let Some(value) = backend.lookup_indexed_global_constant(short)
    {
        return Some(value);
    }
    // Last: an import, which is the one spelling neither the enclosing
    // namespace nor the global scope accounts for.  The file's import
    // tables are read from the source here because a type operand carries
    // no offset to resolve against, and they are read only once every
    // cheaper candidate has missed — a docblock naming an imported
    // constant is rare, and the walk runs over the already-parsed AST.
    //
    // A `use const` alias may arrive qualified with the enclosing
    // namespace, the way a bare name in a type is read as a class.
    let ns = ctx.current_class.and_then(|c| c.file_namespace.as_deref());
    let alias = match ns.and_then(|ns| name.strip_prefix(ns)?.strip_prefix('\\')) {
        Some(alias) if !alias.contains('\\') => alias,
        _ => name,
    };
    let imported = match backend
        .parse_symbol_imports(ctx.content)
        .constants
        .remove(alias)
    {
        Some(fqn) => fqn,
        None => {
            let (first, rest) = name.split_once('\\')?;
            let use_map = backend.parse_use_statements(ctx.content);
            let prefix = crate::names::lookup_import(&use_map, first)?;
            format!("{prefix}\\{rest}")
        }
    };
    backend.lookup_indexed_global_constant(&imported)
}

/// The substitution map that resolves every constant operand a set of types
/// reads through an unevaluated type operator.
///
/// Names in `skip` are left out: a surviving operator's operand is either a
/// constant or a template parameter, and looking a template parameter up as
/// a constant only wastes work.
pub(super) fn constant_operand_subs<'t>(
    types: impl Iterator<Item = &'t PhpType>,
    skip: &[Atom],
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut operands = Vec::new();
    for ty in types {
        if ty.contains_unevaluated_operator() {
            ty.unevaluated_operator_operands(&mut operands);
        }
    }

    let mut subs: HashMap<String, PhpType> = HashMap::new();
    for operand in operands {
        if subs.contains_key(&operand) || skip.iter().any(|p| p.as_str() == operand) {
            continue;
        }
        if let Some(shape) = constant_operand_shape(&operand, ctx) {
            subs.insert(operand, shape);
        }
    }
    subs
}

/// Evaluate the type operators a declared type reads through a constant.
///
/// `key-of<ID_TABLE>` on a parameter and `value-of<ID_TABLE>` on a return
/// describe a set of values as concrete as any written-out union, but the
/// docblock parser only saw a name it could not read and left the operator
/// standing. This reads the constant behind the name and finishes the
/// operator, so the type constrains its call sites like the union it is.
///
/// Every consumer of a declared parameter or return type goes through here,
/// not just the template path: a constant is no less readable from a
/// signature that declares no `@template`.
///
/// Returns `None` when the type has no unevaluated operator, when no operand
/// is a constant we can read, or when reading it changed nothing — in each
/// case the caller keeps the type it already has.
pub(crate) fn evaluate_constant_operands(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    if !ty.contains_unevaluated_operator() {
        return None;
    }
    let subs = constant_operand_subs(std::iter::once(ty), &[], ctx);
    let evaluated = if subs.is_empty() {
        ty.clone()
    } else {
        ty.substitute(&subs)
    };
    let evaluated = if evaluated.contains_unevaluated_operator() {
        evaluate_enum_value_of(&evaluated, ctx)
    } else {
        evaluated
    };
    (evaluated != *ty).then_some(evaluated)
}

/// Finish every `value-of<…>` in `ty` whose operand is a backed enum or one
/// of its cases, re-evaluating the operators around it.
///
/// `value-of<Suit::Hearts>` is the case's backing value and `value-of<Suit>`
/// the union of every case's, which is what a template bound to an enum
/// case reads a shape through (`Data[value-of<T>]` with `T` bound to
/// `Target::DASHBOARD`).  An operand that is not a backed enum, or a case
/// with no backing value, leaves the operator standing.
fn evaluate_enum_value_of(ty: &PhpType, ctx: &ResolutionCtx<'_>) -> PhpType {
    if !ty.contains_unevaluated_operator() {
        return ty.clone();
    }
    let recurse = |inner: &PhpType| evaluate_enum_value_of(inner, ctx);
    match ty.kind() {
        TypeKind::ValueOf(operand) => {
            enum_backing_values(operand, ctx).unwrap_or_else(|| PhpType::value_of(recurse(operand)))
        }
        TypeKind::KeyOf(operand) => crate::php_type::evaluate_key_of(&recurse(operand)),
        TypeKind::IndexAccess(base, index) => {
            crate::php_type::evaluate_index_access(&recurse(base), &recurse(index))
        }
        _ => ty.map_children(&recurse),
    }
}

/// The backing value of the enum case `operand` names (`Suit::Hearts`), or
/// the union of every case's for a backed enum named by itself.
fn enum_backing_values(operand: &PhpType, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let name: &str = match operand.kind() {
        TypeKind::Named(name) => name,
        TypeKind::Raw(raw) => raw,
        _ => return None,
    };
    let (class_part, case) = match name.rsplit_once("::") {
        Some((class_part, case)) => (class_part, Some(case)),
        None => (name, None),
    };
    let class = load_operand_class(class_part, ctx)?;
    if class.kind != crate::types::ClassLikeKind::Enum {
        return None;
    }
    let mut values: Vec<PhpType> = Vec::new();
    for constant in class.constants.iter().filter(|c| c.is_enum_case) {
        if case.is_some_and(|case| constant.name != case) {
            continue;
        }
        let value = crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
            constant.enum_value.as_deref()?,
        )?;
        if !values.contains(&value) {
            values.push(value);
        }
    }
    match values.len() {
        0 => None,
        1 => values.pop(),
        _ => Some(PhpType::union(values)),
    }
}

/// The class a type operand's `Foo` or `Foo::BAR` names.
///
/// The declaring file's parse already qualified the name against its own
/// imports and namespace, so it is read as a fully-qualified name: the call
/// site's imports have no say in what a callee's docblock meant. Only the
/// relative keywords are left, and those read the class in scope.
fn load_operand_class(class_part: &str, ctx: &ResolutionCtx<'_>) -> Option<Arc<ClassInfo>> {
    let class_name = crate::class_lookup::resolve_class_keyword(class_part, ctx.current_class);
    let class_name = class_name
        .as_deref()
        .unwrap_or_else(|| class_part.strip_prefix('\\').unwrap_or(class_part));
    crate::class_lookup::find_class_by_fqn(ctx.all_classes, class_name)
        .cloned()
        .or_else(|| (ctx.class_loader)(class_name))
}
