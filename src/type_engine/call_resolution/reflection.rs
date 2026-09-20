//! Typing a property read through the Reflection API.
//!
//! `ReflectionClass::getProperty()` is declared to return a bare
//! `ReflectionProperty` and `ReflectionProperty::getValue()` a bare
//! `mixed`, so a reflected read loses the type the property itself
//! declares. No `@return` annotation can recover it: the result's type
//! depends on the *value* of the name handed to `getProperty()`, which is
//! not something a type expression can name.
//!
//! The rules here close that gap for the case where the name is a literal
//! and the reflected class is known. `getProperty('shell')` on a
//! `ReflectionClass<Configuration>` yields
//! `ReflectionProperty<Configuration, 'shell'>`, carrying the class and
//! the name on the value so they survive an assignment, and `getValue()`
//! on that reads the declared type of `Configuration::$shell`.
//! `new ReflectionProperty(Configuration::class, 'shell')` is the same
//! value written another way, so it yields the same type.
//!
//! `ReflectionProperty` declares no `@template` of its own -- the pair of
//! type arguments is ours, produced only here and read only here. Every
//! other consumer sees the base class it always did, which is why the
//! generic form travels through the shared pipeline unchanged.

use crate::php_type::{PhpType, TypeKind, is_builtin_non_class_type};
use crate::type_engine::resolver::ResolutionCtx;
use crate::types::{ClassInfo, ResolvedType};

/// The class the rules here reflect over, spelled as its FQN carries it.
const REFLECTION_PROPERTY: &str = "ReflectionProperty";

/// Whether a method name is one the rules below can say anything about.
///
/// Both call-resolution entry points check this before splitting call
/// arguments, so a method call that has nothing to do with reflection
/// costs one string comparison.
pub(crate) fn is_reflected_property_call(method_name: &str) -> bool {
    matches!(method_name, "getProperty" | "getValue")
}

/// Whether an instantiated class is one the `new`-expression rule below
/// can say anything about.
///
/// Both `new`-expression resolution paths check this before pulling the
/// constructor's arguments out of the source, so an unrelated `new` costs
/// one string comparison.
pub(crate) fn is_reflected_property_class(class_fqn: &str) -> bool {
    class_fqn == REFLECTION_PROPERTY
}

/// The class a reflection value reflects, when it is a resolvable class.
///
/// `ReflectionObject` is included because it is `ReflectionClass<T>` with
/// the class-name spelling of the constructor argument taken away (see the
/// `ReflectionObject` stub patch), so both carry the reflected class in
/// the same slot.
fn reflected_class_arg(ty: &PhpType) -> Option<&PhpType> {
    let TypeKind::Generic(generic) = ty.kind() else {
        return None;
    };
    if !matches!(
        generic.name.as_str(),
        "ReflectionClass" | "ReflectionObject"
    ) {
        return None;
    }
    let subject = generic.args.first()?;
    // An unbound `T` erases to its `object` bound, which names no class to
    // look a property up on.
    match subject.kind() {
        TypeKind::Named(name) if !is_builtin_non_class_type(name) => Some(subject),
        _ => None,
    }
}

/// Resolve a `getProperty()` / `getValue()` call on a reflection value to
/// the type the reflected property declares.
///
/// `arg_texts` are the call's arguments as written. Returns `None` for a
/// receiver that is not a reflection value with a known class, and for a
/// `getProperty()` whose name argument is not a string literal -- in each
/// case the stub's own return type stands.
pub(crate) fn resolve_reflected_property_at_call(
    method_name: &str,
    arg_texts: &[&str],
    receiver: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    match method_name {
        "getProperty" => reflect_property(arg_texts, receiver, ctx),
        "getValue" => reflected_property_type(receiver, ctx),
        _ => None,
    }
}

/// `ReflectionClass<C>::getProperty('name')` → `ReflectionProperty<C, 'name'>`.
fn reflect_property(
    arg_texts: &[&str],
    receiver: &[ResolvedType],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let subject = receiver
        .iter()
        .find_map(|rt| reflected_class_arg(&rt.type_string))?;
    let arg = crate::call_args::text_arg_value(arg_texts.first()?);
    let name = reflected_property_name(arg, ctx)?;
    Some(reflected_property(subject.clone(), &name))
}

/// Resolve `new ReflectionProperty(C::class, 'name')` to
/// `ReflectionProperty<C, 'name'>` — the value
/// `ReflectionClass::getProperty('name')` builds, written the other way.
///
/// The binding cannot come from the constructor's own docblock:
/// `class-string|object $class` would name the class through the ordinary
/// `@template` machinery, but `$property` is a string literal, and a
/// literal only binds to a `@template` whose bound is a type operator.
///
/// `class` is the `ReflectionProperty` [`ClassInfo`] the `new` resolved
/// to; the arguments are bound to its constructor's parameters by PHP's
/// rules so a named argument reaches the parameter it targets. Callers
/// guard on [`is_reflected_property_class`] first, so this does not
/// re-check what was instantiated.
pub(crate) fn resolve_reflected_property_at_new(
    class: &ClassInfo,
    arg_texts: &[&str],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let ctor = class.get_method("__construct")?;
    let bound = crate::call_args::bind_text_args_to_params(&ctor.parameters, arg_texts);
    let subject = reflected_class_of_arg(bound.first()?.as_deref()?, ctx)?;
    let name = reflected_property_name(bound.get(1)?.as_deref()?, ctx)?;
    Some(reflected_property(subject, &name))
}

/// The pair of type arguments the two rules above agree on.
fn reflected_property(subject: PhpType, name: &str) -> PhpType {
    PhpType::generic(
        REFLECTION_PROPERTY,
        vec![subject, PhpType::literal_string_value(name)],
    )
}

/// The class a constructor's `class-string|object $class` argument names,
/// when it names exactly one.
///
/// A quoted class name says as much as `Foo::class` does, which is why the
/// shared `class-string<T>` binding does the reading rather than the
/// argument's resolved type alone.
fn reflected_class_of_arg(arg: &str, ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let bound = crate::type_engine::variable::rhs_resolution::class_string_inner_binding(arg, ctx)?;
    // A union of classes, or a scalar that names no class, leaves nothing
    // to look a property up on.
    matches!(bound.kind(), TypeKind::Named(name) if !is_builtin_non_class_type(name))
        .then_some(bound)
}

/// The property name an argument fixes, when it fixes exactly one.
///
/// The name is usually written out, but a variable holding it says just as
/// much once it resolves to a single literal — which is what an accessor
/// that forwards its own `string $property` parameter gives, read from the
/// call site that decided it.
fn reflected_property_name(arg: &str, ctx: &ResolutionCtx<'_>) -> Option<String> {
    match crate::text_scan::unquote_php_string(arg) {
        Some(name) => Some(name.to_string()),
        None => literal_string_value(&crate::Backend::resolve_arg_text_to_type(arg, ctx)?),
    }
}

/// The one string a type stands for, when it stands for exactly one.
fn literal_string_value(ty: &PhpType) -> Option<String> {
    let TypeKind::Literal(literal) = ty.kind() else {
        return None;
    };
    Some(literal.string_content()?.into_owned())
}

/// `ReflectionProperty<C, 'name'>::getValue()` → the declared type of
/// `C::$name`.
///
/// The property is looked up on the fully resolved class so that a
/// reflected read of an inherited or trait-supplied property types the
/// same as a direct one; `ReflectionClass::getProperty()` finds those too.
fn reflected_property_type(receiver: &[ResolvedType], ctx: &ResolutionCtx<'_>) -> Option<PhpType> {
    let (class_name, property) = receiver.iter().find_map(|rt| {
        let TypeKind::Generic(generic) = rt.type_string.kind() else {
            return None;
        };
        if generic.name.as_str() != "ReflectionProperty" || generic.args.len() != 2 {
            return None;
        }
        let TypeKind::Named(class_name) = generic.args[0].kind() else {
            return None;
        };
        let TypeKind::Literal(name) = generic.args[1].kind() else {
            return None;
        };
        Some((*class_name, name.string_content()?.into_owned()))
    })?;

    let class = (ctx.class_loader)(&class_name)?;
    let resolved = crate::virtual_members::resolve_class_fully_maybe_cached(
        &class,
        ctx.class_loader,
        ctx.resolved_class_cache,
    );
    resolved.get_property(&property)?.type_hint.clone()
}
