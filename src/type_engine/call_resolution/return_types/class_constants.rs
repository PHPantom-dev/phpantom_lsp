//! Typing `ClassName::MEMBER` reads: enum cases and their `name`/`value`,
//! class constants by their declared type or folded initializer, and the
//! global constants an initializer refers to.

use std::sync::Arc;

use crate::Backend;
use crate::atom::atom;
use crate::class_lookup::{Fqn, is_class_keyword, is_self_or_static};
use crate::php_type::PhpType;
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::*;

use super::literals::qualify_class_keyword;

/// Resolve a `ClassName::Member` expression to a type.
///
/// Handles enum cases (`MyEnum::Case` → `MyEnum`) and class constants
/// (`Foo::BAR` → the constant's type hint, or the type inferred from
/// the constant's initializer value for untyped constants).  Of the
/// expressions that continue past the member with `->`, only a case's
/// `->value` / `->name` is answered.
pub(crate) fn resolve_static_access_type(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    if text.contains("->") {
        return resolve_enum_case_property_text(text, ctx);
    }
    let (class_part, _member) = text.split_once("::")?;

    // Only accept identifier-like class names (no `$var::`, no whitespace).
    if class_part.is_empty()
        || class_part.starts_with('$')
        || !class_part
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
    {
        return None;
    }

    let cls = static_access_class(class_part, ctx)?;

    // Enum cases resolve to the enum type itself.  A constant declared on
    // the enum is read like any other class constant below.
    if cls.kind == ClassLikeKind::Enum
        && cls
            .constants
            .iter()
            .find(|c| c.name == _member)
            .is_none_or(|c| c.is_enum_case)
    {
        return Some(PhpType::named(cls.fqn()));
    }

    // Class constants: use the declared type hint when available,
    // otherwise infer a type from the initializer value.
    let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
        &cls,
        ctx.file.class_loader,
        ctx.resolved_class_cache,
    );
    class_constant_type(&merged, _member, ctx)
}

/// The literal a `->name` or `->value` read on a named enum case holds:
/// the case's own name, or the value it is backed by.
///
/// `None` when `enum_cls` is not an enum, `case` is not one of its cases,
/// or `property` is neither of the two.
pub(crate) fn enum_case_property_literal(
    enum_cls: &ClassInfo,
    case: &str,
    property: &str,
) -> Option<PhpType> {
    if enum_cls.kind != ClassLikeKind::Enum {
        return None;
    }
    let constant = enum_cls
        .constants
        .iter()
        .find(|c| c.is_enum_case && c.name == case)?;
    match property {
        "name" => Some(PhpType::literal_string_raw(format!("'{case}'"))),
        "value" => crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value(
            constant.enum_value.as_deref()?,
        ),
        _ => None,
    }
}

/// [`enum_case_property_literal`] for an expression given as text,
/// `Enum::CASE->value` or `Enum::CASE->name`.
fn resolve_enum_case_property_text(text: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let is_name = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
    };
    let (object, property) = text.split_once("->")?;
    let (class_part, case) = object.split_once("::")?;
    if !is_name(class_part) || !is_name(case) || !is_name(property) {
        return None;
    }
    let cls = static_access_class(class_part, ctx)?;
    enum_case_property_literal(&cls, case, property)
}

/// The class `class_part` of a `Class::member` expression names: `self`,
/// `static` and `parent` the way they read inside the current class, and
/// any other name as a class named in code.
fn static_access_class(class_part: &str, ctx: &ResolutionCtx<'_>) -> Option<Arc<ClassInfo>> {
    let class_name = if is_self_or_static(class_part) {
        ctx.current_class?.fqn().to_string()
    } else {
        ctx.written_names().resolve_class(class_part)
    };
    (ctx.file.class_loader)(Fqn::new(
        class_name.strip_prefix('\\').unwrap_or(&class_name),
    ))
}

/// The type a class constant holds, read from its initializer where that
/// can be folded and from its declared type otherwise.
///
/// `merged` is the class the constant is looked up on, with inherited
/// members merged in.
pub(crate) fn class_constant_type(
    merged: &ClassInfo,
    member: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let constant = merged.constants.iter().find(|c| c.name == member)?;
    // Infer the value type from the initializer so template params bind
    // to the constant's value (e.g. `int`) rather than the owning class.
    //
    // A declared type (PHP 8.3's `const int NAME = …`) says what the
    // constant may hold, not what it does hold, so the initialiser is
    // still the sharper answer and is read first. It only stands in for
    // the declaration when it refines it.
    //
    // Reading the initialiser can come back here (`const A = [self::A];`,
    // or two constants naming each other), so the whole read is claimed
    // under a key of its own; `folded_class_constant_type` claims the
    // plain `Class::NAME` one for its own fold.
    if let Some(ref val) = constant.value
        && let Some(_guard) = crate::type_engine::type_ops::const_fold::FoldGuard::acquire(
            &format!("{}::{} initializer", merged.fqn(), member),
        )
    {
        // The initialiser was written in the declaring class's file, so the
        // reader's imports have no say in it: the classes it names were
        // qualified against that file's imports when it was parsed, and
        // `self`/`parent` are read against the class that declares the
        // constant. Only an initialiser naming a class constant can depend
        // on which class that is, so the hierarchy is searched for no other.
        let declaring = val
            .contains("::")
            .then(|| constant_declaring_class(merged, constant, ctx.file.class_loader))
            .flatten();
        let owner = declaring.as_deref().unwrap_or(merged);
        let ctx = &ResolutionCtx {
            file: ctx.file.with_imports(None),
            current_class: Some(owner),
            ..*ctx
        };
        let resolve = |text: &str| {
            Backend::resolve_arg_text_to_type(&qualify_class_keyword(text, owner), ctx)
        };
        let inferred =
            crate::type_engine::variable::rhs_resolution::infer_type_from_constant_value_resolved(
                val, &resolve,
            )
            .or_else(|| folded_class_constant_type(owner, member, val, ctx))
            // An initialiser that is itself `Foo::class` or `Class::Case` is
            // not a literal, so the folding above skips it. `Foo::class` is
            // read first, as an enum's `::class` is not one of its cases.
            .or_else(|| {
                let qualified = qualify_class_keyword(val, owner);
                class_name_literal_type(&qualified)
                    .or_else(|| resolve_static_access_type(&qualified, ctx))
            });
        // The initialiser is one fixed value whichever class the constant
        // is read through, so a declared `static` (legal on an enum's
        // constant) is checked as the class it is declared on.
        if let Some(ty) = inferred.filter(|ty| {
            constant
                .type_hint
                .as_ref()
                .is_none_or(|hint| ty.is_subtype_of(&hint.replace_self_bound(&owner.fqn(), None)))
        }) {
            return Some(ty);
        }
    }
    constant.type_hint.clone()
}

/// The class `self` names inside `constant`'s initialiser, when that is an
/// ancestor of `merged` rather than `merged` itself.
///
/// The declaring class is the nearest one, in PHP's lookup order, whose own
/// constants include one of that name, which is the declaration the merge
/// kept. The name is compared rather than the `Arc`: `merged` may come from
/// the resolved-class cache, which outlives a reparse that leaves the class
/// signatures as they were, so its constants can be older copies of the ones
/// the loader returns now. A constant declared on a trait is the class's
/// that uses the trait, as PHP copies it in. `None` when `merged` declares
/// the constant itself, or when no ancestor can be shown to.
fn constant_declaring_class(
    merged: &ClassInfo,
    constant: &Arc<ConstantInfo>,
    class_loader: crate::inheritance::ancestry::ClassLoader<'_>,
) -> Option<Arc<ClassInfo>> {
    use crate::inheritance::ancestry::{ancestors, find_declaring_ancestor, find_declaring_trait};

    let declares = |c: &ClassInfo| c.constants.iter().any(|k| k.name == constant.name);
    let declares_or_uses = |c: &ClassInfo| {
        declares(c) || find_declaring_trait(&c.used_traits, class_loader, &declares).is_some()
    };
    // `merged` carries every inherited constant, so whether it declares this
    // one is read off the class as it was parsed.
    if class_loader(Fqn::new(&merged.fqn())).is_none_or(|own| declares_or_uses(&own)) {
        return None;
    }
    if let Some((_, parent)) = ancestors(merged, class_loader).find(|(_, p)| declares_or_uses(p)) {
        return Some(parent);
    }
    find_declaring_ancestor(merged, class_loader, &declares)
        .map(|(_, iface)| iface)
        .filter(|iface| iface.kind == ClassLikeKind::Interface)
}

/// The type of `text` when it is `Foo::class` with the class spelled in
/// full: exactly the name of that class, as `$x = Foo::class;` is typed in
/// code.
///
/// The class need not exist, since PHP names it without loading it.
fn class_name_literal_type(text: &str) -> Option<PhpType> {
    let (class, constant) = text.split_once("::")?;
    if !constant.eq_ignore_ascii_case("class")
        || is_class_keyword(class)
        || !class
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '\\')
    {
        return None;
    }
    let class = class.strip_prefix('\\').unwrap_or(class);
    (!class.is_empty()).then(|| PhpType::class_name_literal(atom(class)))
}

/// The literal value an untyped class constant holds, folded from an
/// initialiser that names other constants (`const FLAGS = JSON_THROW_ON_ERROR;`,
/// `const COMBO = A | B;`).
///
/// `class` is the class that declares the constant, so `self::` and
/// `parent::` inside the initialiser are read as PHP binds them.
pub(crate) fn folded_class_constant_type(
    class: &ClassInfo,
    const_name: &str,
    value: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolve =
        |text: &str| Backend::resolve_arg_text_to_type(&qualify_class_keyword(text, class), ctx);
    let key = format!("{}::{}", class.fqn(), const_name);
    crate::type_engine::type_ops::const_fold::folded_constant_type(&key, value, &resolve)
}

/// The literal value a global constant holds, folded from an initialiser that
/// names other constants (`const FLAGS = JSON_THROW_ON_ERROR;`, `define('MASK',
/// A | B)`).
pub(crate) fn folded_global_constant_type(
    name: &str,
    value: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let resolve = |text: &str| Backend::resolve_arg_text_to_type(text, ctx);
    crate::type_engine::type_ops::const_fold::folded_constant_type(name, value, &resolve)
}
