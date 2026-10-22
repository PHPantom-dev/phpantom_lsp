//! Diagnostic subject resolution: telling apart why a subject resolved to
//! no class (a scalar, `mixed`, a class name nothing can load, or nothing
//! at all), so a member-access diagnostic can say which.

use std::sync::Arc;

use crate::atom::atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::inheritance::resolve_property_type_hint;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::*;
use crate::virtual_members::resolve_class_fully_maybe_cached;

use super::context::resolved_to_arcs;
use super::{
    ResolutionCtx, resolve_static_owner_class, resolve_target_classes, resolve_target_classes_expr,
};

/// Whether every non-null member of `ty` is the bare `string` type.
///
/// `$var::method()` is valid PHP when `$var` holds a class name at
/// runtime — the string names the class — so a `string`-only subject
/// accessed via `::` can only be "unverifiable", never a scalar-access
/// error the way `$var->method()` on a scalar is.  Other scalars
/// (`int`, `bool`, …) can never be a class name, so they keep reporting
/// as scalar access.
fn is_all_string_type(ty: &PhpType) -> bool {
    match ty.kind() {
        TypeKind::Named(name) => name.eq_ignore_ascii_case("string"),
        TypeKind::Nullable(inner) => is_all_string_type(inner),
        TypeKind::Union(members) => members
            .iter()
            .filter(|m| !m.is_null())
            .all(is_all_string_type),
        _ => false,
    }
}

/// The outcome of resolving a subject for diagnostic purposes.
///
/// [`resolve_target_classes`] only returns `Vec<Arc<ClassInfo>>` and
/// silently drops scalar types and type-string-only entries.
/// Diagnostics need to know *why* resolution returned empty — was the
/// subject a scalar type (runtime crash), an unresolvable class name
/// (likely typo / missing import), or truly untyped?  This enum
/// carries that distinction so the diagnostic collector can emit the
/// right message and severity.
///
/// ## Architectural invariant
///
/// Every `SubjectOutcome` **must** be derived from the same resolution
/// pass that completion and hover use.  Re-resolving a variable
/// through a secondary helper (e.g. `resolve_variable_type`)
/// bypasses narrowing (instanceof, assert, ternary, `&&`) and
/// produces false positives.  See [`resolve_subject_outcome`] for
/// how this is enforced for each subject variant.
#[derive(Clone, Debug)]
pub(crate) enum SubjectOutcome {
    /// Subject resolved to one or more classes.
    Resolved(Vec<Arc<ClassInfo>>),
    /// Subject resolved to a scalar type — member access is always a
    /// runtime crash.  The `PhpType` is the resolved scalar type
    /// (e.g. `int`, `string`, `bool|int`) with null stripped.
    Scalar(PhpType),
    /// Subject resolved to a class name that couldn't be loaded.
    UnresolvableClass(PhpType),
    /// Subject resolved to `mixed`.
    ///
    /// Distinct from [`Untyped`](Self::Untyped): `mixed` is an answer, not
    /// the absence of one. It is what an undocblocked `array`'s elements,
    /// a `@return mixed` accessor and a `mixed` parameter all hold, so the
    /// gap is in what the code says about the value rather than in what
    /// the type engine could work out. A member on it is unverifiable
    /// either way, but the two are worth telling apart: one is a note
    /// about the codebase's annotations, the other a report of our own
    /// resolution falling short.
    Mixed,
    /// Subject type could not be resolved — no class information
    /// available.
    Untyped,
}

/// Resolve a subject to a [`SubjectOutcome`] in a single pass.
///
/// This is the unified entry point for diagnostic subject resolution.
/// It resolves the subject to `Vec<ResolvedType>` (the same pipeline
/// used by completion and hover) and classifies the result:
///
///   - If any entry has `class_info`, return `Resolved`.
///   - If all entries are primitive scalars, return `Scalar`.
///   - If a type string refers to an unloadable class, return
///     `UnresolvableClass`.
///   - If the result is empty, return `Untyped`.
pub(crate) fn resolve_subject_outcome(
    subject: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> SubjectOutcome {
    let resolved = resolve_target_classes(subject, access_kind, ctx);
    if !resolved.is_empty() {
        // ── Check for class-bearing entries ──────────────────────
        let arced: Vec<Arc<ClassInfo>> = ResolvedType::into_arced_classes(resolved.clone());
        if !arced.is_empty() {
            return SubjectOutcome::Resolved(arced);
        }

        // ── All entries are type-string-only (no class info) ────
        let joined = ResolvedType::types_joined(&resolved);

        // `mixed` is a resolved type, and one that absorbs whatever stands
        // beside it, so it is answered before the narrower classifications
        // below get a chance to describe only part of the value.
        if joined.contains_mixed() {
            return SubjectOutcome::Mixed;
        }

        // Pure scalar — member access is a runtime crash, unless this
        // is a `::` access on a `string`-only subject (see
        // `is_all_string_type`), which is unverifiable rather than
        // wrong.
        if joined.all_members_primitive_scalar() {
            let scalar = joined.non_null_type().unwrap_or_else(|| joined.clone());
            if access_kind != AccessKind::DoubleColon || !is_all_string_type(&scalar) {
                return SubjectOutcome::Scalar(scalar);
            }
        }

        // stdClass / object — synthetic resolution.
        if resolved
            .iter()
            .any(|rt| rt.type_string.is_named_ci("stdclass") || rt.type_string.is_object())
        {
            let synthetic = Arc::new(ClassInfo {
                name: crate::atom::atom("stdClass"),
                ..ClassInfo::default()
            });
            return SubjectOutcome::Resolved(vec![synthetic]);
        }

        // Non-scalar, non-class type — check for unresolvable class.
        if let Some(unresolved) = check_unresolvable_class_name(&joined, ctx.file.class_loader) {
            return SubjectOutcome::UnresolvableClass(unresolved);
        }
        return SubjectOutcome::Untyped;
    }

    // ── Result is empty — classify why ──────────────────────────
    let expr = SubjectExpr::parse(subject);

    // For call expressions, check the raw return type hint.
    if let SubjectExpr::CallExpr {
        callee,
        args_text: _,
    } = &expr
    {
        if let Some(scalar) = resolve_call_scalar_return(callee, access_kind, ctx) {
            return SubjectOutcome::Scalar(scalar);
        }
        // A declared `mixed` is why this resolved to no class, and saying
        // so is the difference between reporting the codebase's missing
        // annotation and reporting our own resolution falling short.
        if call_returns_mixed(callee, access_kind, ctx) {
            return SubjectOutcome::Mixed;
        }
        // A call declared `object|null` resolves to no candidate at all,
        // unlike the native `?object` which is caught above.
        if declared_call_return_type(callee, access_kind, ctx, &is_nullable_object).is_some() {
            return SubjectOutcome::Resolved(vec![Arc::new(ClassInfo {
                name: crate::atom::atom("stdClass"),
                ..ClassInfo::default()
            })]);
        }
        // Try unresolvable class detection for function calls.
        if let SubjectExpr::FunctionCall(fn_name) = callee.as_ref()
            && let Some(fl) = ctx.function_loader
            && let Some(func_info) = fl(fn_name.as_str(), 0)
            && let Some(ref raw_type) = func_info.return_type
            && let Some(unresolved) = check_unresolvable_class_name(raw_type, ctx.file.class_loader)
        {
            return SubjectOutcome::UnresolvableClass(unresolved);
        }
    }

    // For property chains, check the property's type hint.
    if let SubjectExpr::PropertyChain { base, property } = &expr {
        let base_arcs = resolved_to_arcs(resolve_target_classes_expr(base, access_kind, ctx));
        for cls in &base_arcs {
            let merged = resolve_class_fully_maybe_cached(
                cls,
                ctx.file.class_loader,
                ctx.resolved_class_cache,
            );
            if let Some(parsed) =
                resolve_property_type_hint(&merged, property, ctx.file.class_loader)
            {
                if parsed.all_members_primitive_scalar() {
                    let scalar = parsed.non_null_type().unwrap_or(parsed);
                    if access_kind != AccessKind::DoubleColon || !is_all_string_type(&scalar) {
                        return SubjectOutcome::Scalar(scalar);
                    }
                    return SubjectOutcome::Untyped;
                }
                return SubjectOutcome::Untyped;
            }
        }
    }

    // For bare variables, try the hover fallback for UnresolvableClass
    // detection only.
    if let SubjectExpr::Variable(var_name) = &expr
        && let Some(resolved_type) =
            crate::type_engine::variable::resolution::resolve_variable_php_type(
                ctx.var_query(var_name),
            )
        && let Some(unresolved) =
            check_unresolvable_class_name(&resolved_type, ctx.file.class_loader)
    {
        return SubjectOutcome::UnresolvableClass(unresolved);
    }

    SubjectOutcome::Untyped
}

/// Check whether a call expression's return type is a scalar.
///
/// Inspects the raw return type hint on the method or function without
/// going through the full class resolution pipeline.
fn resolve_call_scalar_return(
    callee: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let hint = declared_call_return_type(callee, access_kind, ctx, &|hint| {
        hint.all_members_primitive_scalar()
    })?;
    Some(hint.non_null_type().unwrap_or(hint))
}

/// Whether a type is `object`, alone or with `null`.
fn is_nullable_object(ty: &PhpType) -> bool {
    ty.non_null_type().as_ref().unwrap_or(ty).is_object()
}

/// Whether the method or function a call names declares `mixed`.
///
/// A call that resolves to no class at all has two very different causes
/// — the declaration said `mixed`, or nothing could be worked out — and
/// only the declaration is on record to tell them apart.
fn call_returns_mixed(
    callee: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> bool {
    declared_call_return_type(callee, access_kind, ctx, &PhpType::contains_mixed).is_some()
}

/// The return type declared on the method or function a call names, when
/// `accept` recognises it.
///
/// Reads the hint straight off the `MethodInfo`/`FunctionInfo` rather than
/// going through the full class resolution pipeline: the callers want to
/// know what the declaration says, not what a resolver can make of it.
/// `accept` is consulted per candidate so a receiver whose type is a union
/// keeps looking past a class that declares something else.
fn declared_call_return_type(
    callee: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
    accept: &dyn Fn(&PhpType) -> bool,
) -> Option<PhpType> {
    match callee {
        // Instance method call: $obj->getAge()
        SubjectExpr::MethodCall { base, method } => {
            let base_arcs = resolved_to_arcs(resolve_target_classes_expr(base, access_kind, ctx));
            declared_return_type_of(&base_arcs, method, ctx, accept)
        }
        // Standalone function call: getInt()
        SubjectExpr::FunctionCall(fn_name) => {
            if let Some(fl) = ctx.function_loader
                && let Some(func_info) = fl(fn_name, 0)
                && let Some(ref hint) = func_info.return_type
                && accept(hint)
            {
                return Some(hint.clone());
            }
            None
        }
        // Static method call: Foo::getInt()
        SubjectExpr::StaticMethodCall { class, method } => {
            let cls = resolve_static_owner_class(class, ctx);
            declared_return_type_of(cls.as_slice(), method, ctx, accept)
        }
        _ => None,
    }
}

/// The first declared return type `accept` recognises among `classes`'
/// `method`, or `None` when it names no such method on any of them.
///
/// `classes` is a slice rather than one class because `$obj->method()`
/// on a union receiver tries each alternative in turn; `accept` lets a
/// caller looking for a specific shape (all-scalar, `mixed`) keep
/// looking past a class whose declaration is something else.
fn declared_return_type_of(
    classes: &[Arc<ClassInfo>],
    method: &str,
    ctx: &ResolutionCtx<'_>,
    accept: &dyn Fn(&PhpType) -> bool,
) -> Option<PhpType> {
    for cls in classes {
        let resolved =
            resolve_class_fully_maybe_cached(cls, ctx.file.class_loader, ctx.resolved_class_cache);
        if let Some(m) = resolved.get_method_ci(method)
            && let Some(ref hint) = m.return_type
            && accept(hint)
        {
            return Some(hint.clone());
        }
    }
    None
}

/// Check whether a raw type string refers to a class that cannot be
/// loaded.
///
/// Returns `Some(class_name)` when the type looks like a class name
/// (not scalar, not a PHPDoc pseudo-type) but the class loader cannot
/// find it.  Returns `None` for scalars, unions, shapes, and types
/// that resolve successfully.
fn check_unresolvable_class_name(
    raw_type: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<PhpType> {
    if raw_type.all_members_scalar() || raw_type.is_mixed() {
        return None;
    }

    let effective = raw_type.non_null_type().unwrap_or_else(|| raw_type.clone());
    let base = effective.base_name()?;

    if class_loader(Fqn::new(base)).is_none() {
        Some(PhpType::named(atom(base)))
    } else {
        None
    }
}
