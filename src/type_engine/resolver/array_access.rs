//! Resolving an array access subject (`$row['page']`, `$c->items()[0]`,
//! `$this->cache['key']`) by walking its brackets through the raw type of
//! its base.

use std::sync::Arc;

use crate::Backend;
use crate::php_type::PhpType;
use crate::type_engine::subject_expr::{BracketSegment, SubjectExpr};
use crate::types::*;

use super::chain_keys::{lookup_scope_for_subject, subject_scope_key};
use super::context::resolved_to_arcs;
use super::{
    ResolutionCtx, is_bare_variable, resolve_static_owner_class, resolve_target_classes_expr,
};

/// Resolve `$base[...]` to the type its brackets select.
///
/// `expr` is the whole access, `base` and `segments` its parts.
pub(super) fn resolve_array_access_subject(
    expr: &SubjectExpr,
    base: &SubjectExpr,
    segments: &[BracketSegment],
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.all_classes;
    let class_loader = ctx.class_loader;

    // Build the scope key using the canonical double-quote
    // format that the forward walker's `expr_to_subject_key`
    // produces (e.g. `$row["page"]`, `$stmts["0"]`).  Integer
    // indices are stringified because PHP normalises them, so
    // `$a[0]` and `$a["0"]` narrow the same subject.
    let scope_key = subject_scope_key(expr);

    // Check if the forward-walker scope has a narrowed type for
    // this array access (e.g. `$row['page']` narrowed via
    // `instanceof`, or `$stmts[0]` after a guard clause).
    if let Some(narrowed) = lookup_scope_for_subject(&scope_key, ctx) {
        return narrowed;
    }

    // When no scope resolver is available (top-level completion),
    // try resolving the full array access key through the forward
    // walker.  This picks up instanceof narrowing on array elements
    // (e.g. `$row['page'] instanceof Page` narrows `$row["page"]`).
    if ctx.scope_var_resolver.is_none() && matches!(base, SubjectExpr::Variable(_)) {
        let resolved = ctx.resolve_variable_at_cursor(&scope_key);
        if !resolved.is_empty() {
            return resolved;
        }
    }

    // When the base is a call expression (e.g. `$c->items()[0]`),
    // resolve the call's raw return type and use it as a candidate
    // for array-segment walking.  This mirrors the variable path
    // but sources the raw type from the method/function signature
    // instead of from docblock annotations or assignments.
    if let SubjectExpr::CallExpr { callee, args_text } = base {
        // Resolve the call's return type with template and generic
        // substitution applied, so that a method declared
        // `@return T[]` with a `class-string<T>` parameter resolves
        // its element type from the call-site argument (e.g.
        // `$a->findChildrenOfType(Foo::class)[0]` → `Foo`).  The
        // un-substituted raw return type is kept as a fallback for
        // callees the hint path doesn't cover.
        let hint = Backend::resolve_call_return_type_hint(callee, args_text, ctx);
        let raw = resolve_call_raw_return_type(callee, args_text, ctx);
        let candidates = hint.into_iter().chain(raw);
        if let Some(resolved) =
            crate::completion::source::helpers::try_chained_array_access_with_candidates(
                candidates,
                segments,
                current_class,
                all_classes,
                class_loader,
            )
        {
            return resolved_array_access_type_to_resolved(
                resolved,
                current_class,
                all_classes,
                class_loader,
            );
        }
        // Neither the substituted hint nor the raw return type had
        // array-shape / generic / iterable annotations covering the
        // bracket access.  Return empty: `call()[i]` is never the
        // same type as `call()`.
        return vec![];
    }

    let base_var = base.to_subject_text();

    // Build candidate raw types from multiple strategies.
    // Each is tried as a complete pipeline (raw type →
    // segment walk → ClassInfo); the first that succeeds
    // through all segments wins.

    // ── Property chain raw type ─────────────────────────
    // When the base is a property chain (e.g. `$this->cache`,
    // `$obj->items`), resolve the owning class and extract
    // the property's raw type hint.  This preserves generic
    // parameters like `array<string, IntCollection>` or
    // `Collection<int, Translation>` that would be lost if
    // we resolved through `type_hint_to_classes_typed` first.
    let property_raw_type: Option<PhpType> = if let SubjectExpr::PropertyChain {
        base: prop_base,
        property,
    } = base
    {
        let owner_arcs = resolved_to_arcs(resolve_target_classes_expr(prop_base, access_kind, ctx));
        owner_arcs.iter().find_map(|cls| {
            crate::inheritance::resolve_property_type_hint(cls, property, class_loader)
        })
    } else {
        None
    };

    // resolve_variable_types is designed for bare `$variable` names;
    // property chains like `$this->query->joins` are handled by the
    // property_raw_type strategy above.  Skip this strategy for
    // non-variable expressions (chains, array access, comparisons,
    // null coalescing, boolean expressions) to avoid polluting
    // the scope cache with unsupported keys.
    let ast_type: Option<PhpType> = if is_bare_variable(&base_var) {
        // When a scope_var_resolver is available (i.e. we are
        // inside the forward walker), read the variable type
        // from the in-progress ScopeState instead of calling
        // resolve_variable_types which would re-enter the
        // forward walker and cause stack overflow.
        if let Some(scope_resolver) = ctx.scope_var_resolver {
            let prefixed = if base_var.starts_with('$') {
                base_var.clone()
            } else {
                format!("${}", base_var)
            };
            let from_scope = scope_resolver(&prefixed);
            if from_scope.is_empty() {
                None
            } else {
                Some(ResolvedType::types_joined(&from_scope))
            }
        } else {
            let resolved = ctx.resolve_variable_at_cursor(&base_var);
            if resolved.is_empty() {
                None
            } else {
                Some(ResolvedType::types_joined(&resolved))
            }
        }
    } else {
        None
    };

    let candidates = property_raw_type.into_iter().chain(ast_type);

    if let Some(resolved) =
        crate::completion::source::helpers::try_chained_array_access_with_candidates(
            candidates,
            segments,
            current_class,
            all_classes,
            class_loader,
        )
    {
        return resolved_array_access_type_to_resolved(
            resolved,
            current_class,
            all_classes,
            class_loader,
        );
    }
    // Segment walk failed — the base type does not have
    // array-shape, generic, or iterable annotations that
    // cover bracket access.  Return empty: `$var['key']` is
    // never the same type as `$var`.
    vec![]
}

/// Package a segment-walked array access type as `ResolvedType`s.
///
/// The walk in [`crate::completion::source::helpers::try_chained_array_access_with_candidates`]
/// answers with whatever type the bracket access resolves to — a class,
/// a scalar (`array{message: string}['message']` → `string`), or
/// anything else. When it names classes, those carry the full type
/// string as a hint; when it doesn't (a scalar, or a shape/generic type
/// with no matching class), the type string alone is preserved so
/// downstream consumers (hover, hint-based hover fallbacks, template
/// binding) still see it rather than nothing at all.
fn resolved_array_access_type_to_resolved(
    resolved: PhpType,
    current_class: Option<&ClassInfo>,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Vec<ResolvedType> {
    let current_class_fqn = current_class.map(|c| c.fqn());
    let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &resolved,
        current_class_fqn.as_deref().unwrap_or(""),
        all_classes,
        class_loader,
    );
    if classes.is_empty() {
        vec![ResolvedType::from_type_string(resolved)]
    } else {
        ResolvedType::from_classes_with_hint(classes, resolved)
    }
}

/// Extract the raw return type string from a call expression's callee.
///
/// Given a `CallExpr`'s callee and arguments, resolves the owning class
/// (for method/static-method calls) or the function info (for standalone
/// functions), finds the matching method/function, and returns its raw
/// return type string (e.g. `"Item[]"`).  This is used by the
/// `ArrayAccess` handler to strip array dimensions and resolve the
/// element type when the base of `[0]` is a call expression.
fn resolve_call_raw_return_type(
    callee: &SubjectExpr,
    _args_text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    match callee {
        SubjectExpr::MethodCall { base, method } => {
            let base_classes =
                resolved_to_arcs(resolve_target_classes_expr(base, AccessKind::Arrow, ctx));
            raw_return_type_with_magic_fallback(&base_classes, method, "__call", ctx)
        }
        SubjectExpr::StaticMethodCall { class, method } => {
            let owner = resolve_static_owner_class(class, ctx);
            let owner = owner.as_slice();
            raw_return_type_with_magic_fallback(owner, method, "__callStatic", ctx)
        }
        SubjectExpr::FunctionCall(fn_name) => {
            if let Some(fl) = ctx.function_loader
                && let Some(func_info) = fl(fn_name, 0)
            {
                return func_info.return_type.clone();
            }
            None
        }
        _ => None,
    }
}

/// The raw return type of `method` on the first of `classes` that
/// declares it, falling back to `magic_name` (`__call`/`__callStatic`)
/// when the method is virtual (from a `@method` tag or `@mixin`) and
/// carries no return type of its own, or is not declared at all.
///
/// A *real* method with no return type does not fall through: it is
/// invoked directly at runtime, never through the magic method, so
/// reading the magic method's return type there would be answering a
/// different call. `classes` is a slice rather than one class because
/// `$obj->method()` on a union receiver tries each alternative in turn.
fn raw_return_type_with_magic_fallback(
    classes: &[Arc<ClassInfo>],
    method: &str,
    magic_name: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    for cls in classes {
        // Use a fully-resolved class so that inherited docblock return
        // types (e.g. `list<Pen>` from an interface or parent) are
        // visible instead of the bare native hint.
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            cls,
            ctx.class_loader,
            ctx.resolved_class_cache,
        );
        if let Some(m) = merged.get_method_ci(method) {
            if let Some(ref ret) = m.return_type {
                return Some(ret.clone());
            }
            if !m.is_virtual {
                continue;
            }
        }
        if let Some(m) = merged.get_method_ci(magic_name)
            && let Some(ref ret) = m.return_type
        {
            return Some(ret.clone());
        }
    }
    None
}
