//! Type resolution for completion subjects.
//!
//! This module contains the core entry points for resolving a completion
//! subject (e.g. `$this`, `self`, `static`, `$var`, `$this->prop`,
//! `ClassName`) to a concrete `ClassInfo` so that the correct completion
//! items can be offered.
//!
//! The resolution logic is split across several sibling modules:
//!
//! - [`super::call_resolution`]: Call expression and callable target
//!   resolution (method calls, static calls, function calls, constructor
//!   calls, signature help, named-argument completion).
//! - [`super::type_resolution`]: Type-hint string to `ClassInfo` mapping
//!   (unions, intersections, generics, type aliases, object shapes).
//! - [`crate::completion::source::helpers`]: Source-text scanning helpers
//!   (closure return types, first-class callable resolution, `new`
//!   expression parsing, array access segment walking).
//! - [`super::variable::resolution`]: Variable type resolution via
//!   assignment scanning and parameter type hints.
//! - [`super::type_ops::condition_facts`]: what instanceof / assert / custom
//!   type guard conditions say about a variable.
//! - [`super::variable::closure_resolution`]: Closure and arrow-function
//!   parameter resolution.
//! - [`crate::inheritance`]: Class inheritance merging (traits, mixins,
//!   parent chain).
//! - [`super::conditional_resolution`]: PHPStan conditional return type
//!   resolution at call sites.
//!
//! Within this module, context types ([`ResolutionCtx`], [`VarResolutionCtx`],
//! [`Loaders`]) and the thread-local chain resolution cache live in
//! [`context`], how the contexts read a class name written in the file in
//! [`written_names`], the keys a subject chain is cached and narrowed under
//! in [`chain_keys`], array access subjects in [`array_access`], `$this` and
//! `$var` subjects in [`variable_subjects`], and the diagnostic
//! classification of a subject that resolved to no class in
//! [`subject_outcome`].

mod array_access_subject;
mod chain_keys;
mod context;
mod subject_outcome;
mod variable_subjects;
mod written_names;

pub(crate) use chain_keys::is_narrowable_call;
pub(crate) use context::{
    ChainCacheGuard, CtxLoaders, FileCtx, FunctionLoaderFn, LendsLoaders, Loaders, OwnedLoaders,
    ResolutionCtx, ScopeVarResolverFn, SourceImports, VarResolutionCtx,
    with_chain_resolution_cache, with_isolated_chain_cache,
};
pub(crate) use subject_outcome::{SubjectOutcome, resolve_subject_outcome};
pub(crate) use variable_subjects::resolve_dynamic_member_name;
pub(in crate::type_engine) use written_names::WrittenNames;

use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::{Fqn, find_class_by_name, is_self_or_static, resolve_class_keyword};
use crate::inheritance::resolve_property_type_hint;
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::*;

use crate::type_engine::variable::class_string_resolution::classes_named_by_type;
use chain_keys::{SpineKeys, lookup_chain_cache, lookup_scope_for_subject};
use context::{CHAIN_CACHE, ChainKey, resolved_to_arcs};
use variable_subjects::{resolve_this_subject, resolve_variable_fallback};

/// Resolve a completion subject to all candidate types, preserving
/// both class info and type strings.
///
/// This is the primary entry point for subject resolution.  It returns
/// `Vec<ResolvedType>` which carries both the structured type string
/// (e.g. `PhpType::named("Collection")`) and the optional `ClassInfo`.
/// Callers that only need classes can call
/// `ResolvedType::into_arced_classes()` on the result.
pub(crate) fn resolve_target_classes(
    subject: &str,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let expr = SubjectExpr::parse(subject);
    resolve_target_classes_expr(&expr, access_kind, ctx)
}

/// Core dispatch for [`resolve_target_classes`], operating on a
/// pre-parsed [`SubjectExpr`].
///
/// A method or property chain nests one node per link, and each link's
/// child is the link before it — a chain, not a tree.  Resolving the
/// outermost link and recursing into its base would therefore cost a stack
/// frame per link, and a generated fluent chain has no length bound, so the
/// spine is collected into a `Vec` and resolved outward from its base
/// instead.
pub(crate) fn resolve_target_classes_expr(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // `spine[0]` is the expression itself, `spine[len - 1]` its base.  Each
    // entry carries the access kind its parent link resolves it with: a
    // method call always reads its receiver through `->`, while a property
    // chain passes its own access kind down.
    let mut spine: Vec<(&SubjectExpr, AccessKind)> = vec![(expr, access_kind)];
    loop {
        let &(node, node_access) = spine.last().expect("spine is seeded with `expr`");
        let next = match node {
            SubjectExpr::CallExpr { callee, .. } => match callee.as_ref() {
                SubjectExpr::MethodCall { base, .. } => Some((base.as_ref(), AccessKind::Arrow)),
                _ => None,
            },
            SubjectExpr::PropertyChain { base, .. } => Some((base.as_ref(), node_access)),
            _ => None,
        };
        match next {
            Some(entry) => spine.push(entry),
            None => break,
        }
    }

    let keys = SpineKeys::new(&spine, ctx);

    // Probe the outermost link inward for an answer that does not depend on
    // the receiver: a cached result, or a type the forward walker narrowed
    // the link's path to.  The first hit is a finished result for that
    // link, so nothing below it needs resolving at all.
    let mut receiver: Option<Vec<ResolvedType>> = None;
    let mut unresolved = spine.len();
    for index in 0..spine.len() {
        let hit = keys
            .cache_key(index)
            .and_then(lookup_chain_cache)
            .or_else(|| {
                keys.scope_key(index)
                    .and_then(|key| lookup_scope_for_subject(key, ctx))
            });
        let Some(hit) = hit else { continue };
        if index == 0 {
            return hit;
        }
        receiver = Some(hit);
        unresolved = index;
        break;
    }

    for index in (0..unresolved).rev() {
        let (node, node_access) = spine[index];
        receiver = Some(resolve_chain_link(
            node,
            node_access,
            keys.cache_key(index),
            receiver.take(),
            ctx,
        ));
    }
    receiver.unwrap_or_default()
}

/// Resolve one link of a receiver spine, reading from and writing to the
/// chain cache under `cache_key` when the link is cacheable.
fn resolve_chain_link(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    cache_key: Option<&ChainKey>,
    receiver: Option<Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let Some(cache_key) = cache_key else {
        return resolve_target_classes_expr_inner(expr, access_kind, receiver, ctx);
    };
    if let Some(hit) = lookup_chain_cache(cache_key) {
        return hit;
    }
    let result = resolve_target_classes_expr_inner(expr, access_kind, receiver, ctx);
    CHAIN_CACHE.with(|cell| {
        if let Some(ref mut cache) = *cell.borrow_mut() {
            cache.entries.insert(cache_key.clone(), result.clone());
        }
    });
    result
}

/// Whether `var_name` is a plain `$variable` name rather than a complex
/// expression (a property/static chain, array access, a comparison or
/// boolean operator) that a scope entry could never match.
///
/// The scope-reading fast path is built for bare variables; anything
/// else falls through to it wastefully, so callers guard on this before
/// taking that path.
fn is_bare_variable(var_name: &str) -> bool {
    !var_name.contains("->")
        && !var_name.contains("::")
        && !var_name.contains('[')
        && !var_name.contains("===")
        && !var_name.contains("&&")
        && !var_name.contains("??")
        && !var_name.contains("||")
}

/// Inner implementation of [`resolve_target_classes_expr`] without
/// chain caching.  The outer function handles cache lookup/store and the
/// spine walk, so `receiver` arrives already resolved for every link but
/// the base.
///
/// Recursion here follows the finite structure of the subject
/// expression plus variable resolution, which carries its own keyed
/// re-entry guards; class resolution cannot re-enter it (cycles are
/// broken inside `resolve_class_fully`), so no depth cap is needed.
fn resolve_target_classes_expr_inner(
    expr: &SubjectExpr,
    access_kind: AccessKind,
    receiver: Option<Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.file.all_classes;
    let class_loader = ctx.file.class_loader;

    match expr {
        // ── Keywords that always mean "current class" ────────────
        SubjectExpr::This => resolve_this_subject(ctx),
        SubjectExpr::SelfKw | SubjectExpr::StaticKw => resolve_self_static_class(ctx)
            .map(ResolvedType::from_arc)
            .into_iter()
            .collect(),

        // ── `parent::` — resolve to the current class's parent ──
        SubjectExpr::Parent => {
            if let Some(cc) = current_class
                && let Some(ref parent_name) = cc.parent_class
            {
                return crate::class_lookup::find_or_load_resolved(
                    parent_name,
                    all_classes,
                    class_loader,
                )
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect();
            }
            vec![]
        }

        // ── Inline array literal with index access ──────────────
        SubjectExpr::InlineArray { elements, .. } => {
            let mut element_types = Vec::new();
            for elem_text in elements {
                let elem = elem_text.trim();
                if elem.is_empty() {
                    continue;
                }
                let elem_expr = SubjectExpr::parse(elem);
                let resolved = resolve_target_classes_expr(&elem_expr, AccessKind::Arrow, ctx);
                ResolvedType::extend_unique(&mut element_types, resolved);
            }
            element_types
        }

        // ── Enum case / static member access ────────────────────
        SubjectExpr::StaticAccess { class, member } => {
            resolve_static_access_subject(class, member, ctx)
        }

        // ── A bare class name, and `new ClassName` without trailing
        //    call parens, are the same reference written two ways ──
        SubjectExpr::ClassName(name) | SubjectExpr::NewExpr { class_name: name } => {
            // Both are source-level references: PHP resolves an
            // unqualified name through the imports, else the current
            // namespace, never a global class of the same short name.
            class_loader(Fqn::new(&ctx.written_names().resolve_class(name)))
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect()
        }

        // ── Call expression ─────────────────────────────────────
        SubjectExpr::CallExpr { callee, args_text } => {
            resolve_call_subject(callee, args_text, receiver, ctx)
        }

        // ── Property chain ──────────────────────────────────────
        SubjectExpr::PropertyChain { base, property } => {
            resolve_property_chain_subject(base, property, access_kind, receiver, ctx)
        }

        // ── Array access on variable or call expression ─────────
        SubjectExpr::ArrayAccess { base, segments } => {
            array_access_subject::resolve_array_access_subject(
                expr,
                base,
                segments,
                access_kind,
                ctx,
            )
        }

        // ── Bare variable ───────────────────────────────────────
        SubjectExpr::Variable(var_name) => resolve_variable_fallback(var_name, access_kind, ctx),

        // ── Callee-only variants (MethodCall, StaticMethodCall,
        //    FunctionCall) should not appear as top-level subjects;
        //    they are wrapped in CallExpr.  If they do appear
        //    (e.g. from a partial parse), treat as class name. ────
        SubjectExpr::MethodCall { .. }
        | SubjectExpr::StaticMethodCall { .. }
        | SubjectExpr::FunctionCall(_) => {
            let text = expr.to_subject_text();
            if let Some(cls) = find_class_by_name(all_classes, &text) {
                return vec![ResolvedType::from_arc(Arc::clone(cls))];
            }
            class_loader(Fqn::new(&text))
                .map(ResolvedType::from_arc)
                .into_iter()
                .collect()
        }
    }
}

/// Resolve `Foo::MEMBER` / `Foo::$prop`: the type a static property or a
/// class constant holds, an enum case, or else the class itself.
fn resolve_static_access_subject(
    class: &str,
    member: &str,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class = ctx.current_class;
    let all_classes = ctx.file.all_classes;
    let class_loader = ctx.file.class_loader;

    // Handle self/static/parent keywords — SubjectExpr::parse
    // produces StaticAccess for "self::MONTH", "static::FOO",
    // etc., but "self"/"static"/"parent" are keywords, not
    // class names, so find_class_by_name / class_loader won't
    // find them.
    let owner_classes: Vec<Arc<ClassInfo>> = if is_self_or_static(class) {
        resolve_self_static_class(ctx).into_iter().collect()
    } else if let Some(parent_name) = resolve_class_keyword(class, current_class) {
        // `parent`, whose name the parser already resolved.
        crate::class_lookup::find_or_load_resolved(&parent_name, all_classes, class_loader)
            .into_iter()
            .collect()
    } else {
        // `Foo::` is a source-level reference: PHP resolves an
        // unqualified name through the imports, else the current
        // namespace, never a global class of the same short name.
        class_loader(Fqn::new(&ctx.written_names().resolve_class(class)))
            .into_iter()
            .collect()
    };

    // When the member is a static property (starts with `$`),
    // resolve to the property's declared type instead of the
    // owning class.  This makes `self::$instance->method()`
    // resolve `method()` on the property's type, not on the
    // class that declares the static property.  A property whose
    // type names no class (`string[]`) still is that type, never
    // the class declaring it.
    if let Some(prop_name) = member.strip_prefix('$') {
        let mut results: Vec<ResolvedType> = Vec::new();
        for cls in &owner_classes {
            let Some(hint) =
                crate::inheritance::resolve_property_type_hint(cls, prop_name, class_loader)
            else {
                continue;
            };
            let resolved = super::type_resolution::type_hint_to_classes_typed(
                &hint,
                &cls.fqn(),
                all_classes,
                class_loader,
            );
            let resolved = if resolved.is_empty() {
                vec![ResolvedType::from_type_string(hint)]
            } else {
                resolved.into_iter().map(ResolvedType::from_arc).collect()
            };
            ResolvedType::extend_unique(&mut results, resolved);
        }
        return results;
    } else {
        // Otherwise the member is an enum case or class constant.
        // Resolve it through the same constant-type path the call
        // resolver uses (`resolve_static_access_type`) instead of
        // returning the owning class outright: a typed or
        // inferred class constant holds whatever type its value
        // is (e.g. an enum case held in a `const Kind TYPED`),
        // not the type of the class that declares the constant.
        // An enum case's own class already resolves to itself
        // through this same path, so no separate case is needed.
        let mut const_results: Vec<ResolvedType> = Vec::new();
        for owner in &owner_classes {
            let text = format!("\\{}::{member}", owner.fqn());
            if let Some(ty) = super::call_resolution::resolve_static_access_type(&text, ctx) {
                let owning_class_name = owner.fqn().to_string();
                let resolved = super::type_resolution::type_hint_to_classes_typed(
                    &ty,
                    &owning_class_name,
                    all_classes,
                    class_loader,
                );
                ResolvedType::extend_unique(
                    &mut const_results,
                    resolved.into_iter().map(ResolvedType::from_arc).collect(),
                );
            }
        }
        if !const_results.is_empty() {
            return const_results;
        }
    }

    owner_classes
        .into_iter()
        .map(ResolvedType::from_arc)
        .collect()
}

/// Resolve a call subject (`$obj->make()`, `Foo::create()`, `helper()`) to
/// the classes it returns, keeping its return type where that says more.
fn resolve_call_subject(
    callee: &SubjectExpr,
    args_text: &str,
    receiver: Option<Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // Narrowing on the call itself (`if ($h->get() instanceof
    // Foo) { $h->get()->m(); }`) has already been consulted by the
    // spine walk in `resolve_target_classes_expr`, which reads it
    // before resolving anything below the call.
    let mut hint: Option<PhpType> = None;
    let classes = Backend::resolve_call_return_types_on_receiver(
        callee,
        args_text,
        receiver,
        ctx,
        Some(&mut hint),
    );

    // Use the raw return type hint only when it actually carries
    // generic args a resolved class can benefit from: either the
    // class declares its own `@template` params, or (a `static<...>`
    // rebind on a class that only fixes its generics via
    // `@extends`) the hint's generic base names one of the
    // resolved classes directly, so the args are its own rebind
    // rather than a leftover from an unrelated wrapper type.  An
    // intersection hint (e.g. a conditional return type's winning
    // branch, `Foo&MockInterface`) is used unconditionally: without
    // it, `classes` is a flat, untagged list of the intersection's
    // members, which `types_joined` cannot tell apart from a union
    // of alternatives.
    if let Some(h) = hint {
        let hint_names_a_class = matches!(h.kind(), TypeKind::Generic(g) if classes
            .iter()
            .any(|c| g.name.as_str() == c.fqn().as_str() || g.name.as_str() == c.name.as_str()));
        let is_intersection = matches!(h.kind(), TypeKind::Intersection(_));
        if is_intersection
            || classes.iter().any(|c| !c.template_params.is_empty())
            || hint_names_a_class
        {
            return ResolvedType::from_classes_with_hint(classes, h);
        }
        // A scalar conditional branch (and the `object`/`?object`
        // escape hatch) has no concrete class to carry it. Surface
        // it as a type-string-only candidate instead of dropping it,
        // so every consumer sees the branch selected at this call
        // site rather than re-reading the callable's broader union.
        if classes.is_empty() && (h.is_object() || h.all_members_primitive_scalar()) {
            return vec![ResolvedType::from_type_string(h)];
        }
    }

    classes.into_iter().map(ResolvedType::from_arc).collect()
}

/// Resolve `$base->property` to the classes its declared type names,
/// each spelled with the generic arguments the type gives it.
fn resolve_property_chain_subject(
    base: &SubjectExpr,
    property: &str,
    access_kind: AccessKind,
    receiver: Option<Vec<ResolvedType>>,
    ctx: &ResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let all_classes = ctx.file.all_classes;
    let class_loader = ctx.file.class_loader;

    // A type the forward walker narrowed this exact property path
    // to has already been consulted by the spine walk in
    // `resolve_target_classes_expr`, which reads it before
    // resolving anything below the property.
    let base_arcs = resolved_to_arcs(
        receiver.unwrap_or_else(|| resolve_target_classes_expr(base, access_kind, ctx)),
    );
    let mut arc_results: Vec<Arc<ClassInfo>> = Vec::new();
    // The generic members of the property's hint, e.g.
    // `Collection<int, Post>`.  The classes resolved from it carry
    // only its base name, so each is given its generic spelling
    // back below.
    let mut generic_hints: Vec<PhpType> = Vec::new();
    for cls in &base_arcs {
        let Some(hint) = resolve_property_type_hint(cls, property, class_loader) else {
            continue;
        };
        let resolved = super::type_resolution::type_hint_to_classes_typed(
            &hint,
            &cls.fqn(),
            all_classes,
            class_loader,
        );
        generic_hints.extend(
            hint.unwrap_nullable()
                .union_members()
                .into_iter()
                .filter(|member| matches!(member.kind(), TypeKind::Generic(_)))
                .cloned(),
        );

        ClassInfo::extend_unique_arc(&mut arc_results, resolved);
    }

    // ── `class-string<T>` unwrapping for `prop::` access ────
    // A property typed `class-string<T>` holds a class name at
    // runtime, not an instance of its own declared type.
    // `resolve_property_types` skips it (it isn't a class
    // type), so when the chain is followed by `::`, resolve
    // static members against `T` instead of leaving
    // `arc_results` empty.
    if arc_results.is_empty() && access_kind == AccessKind::DoubleColon {
        for cls in &base_arcs {
            if let Some(raw_type) = resolve_property_type_hint(cls, property, class_loader) {
                let classes =
                    classes_named_by_type(&raw_type, &cls.fqn(), all_classes, class_loader);
                ClassInfo::extend_unique_arc(&mut arc_results, classes);
            }
        }
    }

    // The generic spelling of each class the hint named.
    let mut resolved = ResolvedType::from_classes(arc_results);
    for rt in &mut resolved {
        let Some(class) = &rt.class_info else {
            continue;
        };
        let fqn = class.fqn();
        if let Some(hint) = generic_hints.iter().find(|hint| {
            hint.base_name()
                .is_some_and(|base| base == fqn.as_str() || crate::util::short_name(&fqn) == base)
        }) {
            rt.type_string = hint.clone();
        }
    }
    resolved
}

// ── Static owner class resolution ───────────────────────────────────

/// Resolve the class that a bare `self`/`static` keyword refers to at
/// the cursor position.
///
/// Normally this is the lexically enclosing class, but inside a
/// closure whose enclosing call site declares `@param-closure-this`
/// (e.g. a Laravel `Macroable` or Carbon `macro()` registration), the
/// runtime binds the closure with the target class as its scope
/// (`Closure::bind`), so `self::` and `static::` refer to the bound
/// target rather than the class that lexically encloses the closure.
fn resolve_self_static_class(ctx: &ResolutionCtx<'_>) -> Option<Arc<ClassInfo>> {
    super::variable::closure_resolution::find_closure_this_override(ctx).or_else(|| {
        let current = ctx.current_class?;
        // The enclosing class is normally one of the file's own, already
        // behind an `Arc`.
        Some(
            ctx.file
                .all_classes
                .iter()
                .find(|cls| std::ptr::eq(cls.as_ref(), current))
                .cloned()
                .unwrap_or_else(|| Arc::new(current.clone())),
        )
    })
}

/// Resolve a static class reference (`self`, `static`, `parent`, or a
/// class name) to its `ClassInfo`.
///
/// Handles the `self`/`static`/`parent` keywords and falls back to
/// `class_loader` then `resolve_target_classes` for named classes.
pub(in crate::type_engine) fn resolve_static_owner_class(
    class: &str,
    rctx: &ResolutionCtx<'_>,
) -> Option<Arc<ClassInfo>> {
    if is_self_or_static(class) {
        resolve_self_static_class(rctx)
    } else if let Some(resolved_name) = resolve_class_keyword(class, rctx.current_class) {
        // parent — load via class_loader so we get the full parent ClassInfo
        (rctx.file.class_loader)(Fqn::new(&resolved_name))
    } else {
        let fqn = rctx.written_names().resolve_class(class);
        (rctx.file.class_loader)(Fqn::new(&fqn)).or_else(|| {
            resolved_to_arcs(resolve_target_classes(
                class,
                crate::AccessKind::DoubleColon,
                rctx,
            ))
            .into_iter()
            .next()
        })
    }
}
