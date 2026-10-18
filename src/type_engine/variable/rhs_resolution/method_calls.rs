//! Method call return-type resolution on a receiver: resolving the
//! receiver, splitting a union or intersection receiver into one owner
//! class per member, and combining what each owner's method returns.

use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};
use crate::virtual_members::laravel::validated_shape;

use crate::type_engine::resolver::VarResolutionCtx;

use super::calls::any_argument_is_never;
use super::function_calls::closure_call_return;
use super::laravel_calls::{
    try_resolve_command_accessor_type, try_resolve_config_method_type,
    try_resolve_request_accessor_type, try_resolve_trans_method_type, try_resolve_validated_shape,
};
use super::owner_method_call::resolve_owner_method_call;
use super::scope_reads::narrowed_subject_from_scope;
use super::{resolve_rhs_expression, resolve_var_types};

/// A method call's receiver, already resolved: the candidate owner classes
/// plus the `ResolvedType` values they came from.
///
/// The full `ResolvedType` is kept alongside the classes so that the
/// receiver's generic type string (e.g. `Builder<Article>`) is available
/// when the method returns `static`/`self`/`$this`.
pub(super) type MethodReceiver = (Vec<Arc<ClassInfo>>, Vec<ResolvedType>);

/// Resolve a method call's object expression to its receiver.
///
/// `$this` resolves to what the scope says it is, else the enclosing class;
/// a bare variable through the variable pipeline (honouring `match(true)`
/// arm narrowing); and anything else — `(new Factory())`, `getService()`, a
/// chain link — by resolving the expression.
pub(super) fn resolve_method_receiver<'b>(
    object: &'b Expression<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> MethodReceiver {
    if let Expression::Variable(Variable::Direct(dv)) = object
        && dv.name == b"$this"
    {
        // The walker seeds `$this` for every body and narrows it like any
        // other variable: a closure bound to a test case, an
        // `assert($this instanceof X)` in a top-level closure, a check
        // naming a subclass, the users a trait's `$this` stands for.  The
        // receiver's own types stay empty so `static` is still bound late,
        // to the class the call runs on.
        let classes = match narrowed_subject_from_scope("$this", object, ctx) {
            Some(narrowed) => ResolvedType::into_arced_classes(narrowed),
            None => {
                crate::class_lookup::find_class_by_fqn(ctx.all_classes, &ctx.current_class.fqn())
                    .map(Arc::clone)
                    .into_iter()
                    .collect()
            }
        };
        return (classes, vec![]);
    }
    // Read directly rather than through `resolve_rhs_expression`, whose
    // self-assignment guard would leave `$node = $node->next()` without a
    // receiver.
    if let Expression::Variable(Variable::Direct(dv)) = object {
        let resolved = resolve_var_types(bytes_to_str(dv.name), ctx, object.span().end.offset);
        let classes = ResolvedType::into_arced_classes(resolved.clone());
        return (classes, resolved);
    }
    let resolved = resolve_rhs_expression(object, ctx);
    let classes = ResolvedType::into_arced_classes(resolved.clone());
    (classes, resolved)
}

/// Resolve a method call (regular or null-safe) from its constituent parts:
/// the object expression (`$this`, a variable, or an arbitrary chained
/// expression), the method selector, and the argument list.
///
/// Both `$obj->method()` and `$obj?->method()` share the same resolution
/// logic — the null-safe operator only affects whether `null` propagates
/// at runtime, not which class the method belongs to.
pub(super) fn resolve_rhs_method_call_inner<'b>(
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    resolve_method_call_on_receiver(object, method, argument_list, None, ctx)
}

/// Resolve a method call whose receiver may already be known.
///
/// A `Some(receiver)` skips resolving `object`, which is how a fluent chain
/// is walked outward from its base without recursing into each link (see
/// `resolve_method_chain` in the parent module).  `object` is still needed
/// for the parts of resolution that read the receiver's *syntax* rather
/// than its type: whether the call forwards late static binding, and which
/// request a Laravel validation shape belongs to.
pub(super) fn resolve_method_call_on_receiver<'b>(
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: &'b ArgumentList<'b>,
    receiver: Option<MethodReceiver>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    if any_argument_is_never(argument_list, ctx) {
        return vec![ResolvedType::from_type_string(PhpType::never())];
    }
    resolve_member_call_on_receiver(object, method, Some(argument_list), receiver, false, ctx)
}

/// [`resolve_method_call_on_receiver`] for either call form.
///
/// `is_static` marks `$obj::method()`, which PHP dispatches on the class of
/// the object `$obj` holds: every rule of an instance call applies, except
/// that a missing method falls back to `__callStatic` rather than `__call`.
///
/// `argument_list` is `None` for a first-class callable (`$obj->m(...)`):
/// what the closure returns is what the method declares, so the rules that
/// read a call's arguments do not apply.
pub(super) fn resolve_member_call_on_receiver<'b>(
    object: &'b Expression<'b>,
    method: &'b ClassLikeMemberSelector<'b>,
    argument_list: Option<&'b ArgumentList<'b>>,
    receiver: Option<MethodReceiver>,
    is_static: bool,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let Some(method_name) = selected_member_name(method, ctx) else {
        return super::runtime_named_member_type();
    };
    if method_name.eq_ignore_ascii_case("call")
        && let Some(argument_list) = argument_list
        && let Some(returned) = closure_call_return(object, argument_list, ctx)
    {
        return returned;
    }
    let (owner_classes, receiver_resolved) =
        receiver.unwrap_or_else(|| resolve_method_receiver(object, ctx));

    let arg_texts = argument_list
        .map(|list| {
            crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                list,
                ctx.content,
            )
        })
        .unwrap_or_default();
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
    let rctx = ctx.as_resolution_ctx();

    // ── Expand union generic receivers ──────────────────────────
    // When the receiver is a union type like `C<A>|C<B>`, the variable
    // resolution pipeline returns a single ResolvedType with a Union
    // type_string and one class_info.  To resolve the method on each
    // branch separately (so `->get()` yields `A|B` not just `A`),
    // expand the union into separate owner entries with per-branch
    // generic substitutions applied.
    let (owner_classes, receiver_resolved) =
        expand_union_generic_owners(owner_classes, receiver_resolved, ctx);

    // A property read through the Reflection API: the name handed to
    // `getProperty()` decides the type, so the stub's `ReflectionProperty`
    // / `mixed` return types are as specific as an annotation can be.
    // Keyed on the receiver's own type arguments rather than on an owner
    // class, since it is the reflected class the result depends on.
    if crate::type_engine::call_resolution::is_reflected_property_call(&method_name)
        && let Some(ty) = crate::type_engine::call_resolution::resolve_reflected_property_at_call(
            &method_name,
            &arg_refs,
            &receiver_resolved,
            &rctx,
        )
    {
        let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &ty,
            "",
            ctx.all_classes,
            ctx.class_loader,
        );
        return if classes.is_empty() {
            vec![ResolvedType::from_type_string(ty)]
        } else {
            ResolvedType::from_classes_with_hint(classes, ty)
        };
    }

    // Laravel validated input: the rules that guard this request describe the
    // array it hands back, so `$data = $request->validated()` gets a shape
    // rather than plain `array`.  Classifying the call does not depend on the
    // receiver, so it happens once rather than once per owner.
    //
    // `validate([…])` reads its rules from its own argument, so it works
    // wherever it is written; every other form asks the scope for them and is
    // a no-op without the server state that reads them.
    let shape_call = validated_shape::shape_bearing_method(&method_name)
        .filter(|call| *call == validated_shape::ShapeCall::Validate || ctx.backend.is_some());

    // Laravel request input: `header('X', '')`, `query()`, `file('photo')`
    // and the rest declare one union spanning every way of calling them,
    // and the arguments say which of those ways this is.  Classified once
    // for the same reason the shape call above is.
    let input_accessor =
        crate::virtual_members::laravel::request_input::input_accessor(&method_name);

    // Each of these reads its answer off the call's arguments, so a
    // first-class callable, which has none, takes the declared type.
    if let Some(argument_list) = argument_list {
        for owner in &owner_classes {
            if let Some(result) =
                try_resolve_config_method_type(&owner.fqn(), &method_name, argument_list, ctx)
            {
                return result;
            }
            if let Some(result) =
                try_resolve_trans_method_type(&owner.fqn(), &method_name, argument_list, ctx)
            {
                return result;
            }
            if let Some(result) =
                try_resolve_command_accessor_type(owner, &method_name, argument_list, ctx)
            {
                return result;
            }
            if let Some(call) = shape_call
                && let Some(shape) =
                    try_resolve_validated_shape(owner, call, object, &arg_refs, ctx)
            {
                return vec![ResolvedType::from_type_string(shape)];
            }
            if let Some(accessor) = input_accessor
                && let Some(result) = try_resolve_request_accessor_type(
                    owner,
                    accessor,
                    &method_name,
                    argument_list,
                    &arg_refs,
                    ctx,
                )
            {
                return result;
            }
        }
    }

    // Laravel factory count state: `create()`/`make()` build a single
    // model, or a collection of them when the chain set a count
    // (`factory(3)`, `count(3)`, `times(3)`).  Laravel declares both
    // outcomes as one `Collection<int, TModel>|TModel` return type, so
    // without this the union survives into every assignment, argument and
    // property write that a factory chain feeds.
    if let Some((classes, hint)) = crate::virtual_members::laravel::resolve_factory_count_return_ast(
        object,
        &method_name,
        &receiver_resolved,
        ctx.content,
        &rctx,
    ) {
        return vec![match classes.first() {
            Some(class) => ResolvedType::from_both_arc(hint, Arc::clone(class)),
            None => ResolvedType::from_type_string(hint),
        }];
    }

    // A fluent factory call (`state()`, `for()`, `hasPosts()`, `count()`)
    // returns the factory itself, so the count state the chain has built
    // up so far travels on to the result — which is what lets a `create()`
    // several statements and one variable later still know what it builds.
    // The argument type is resolved lazily: only a non-literal argument to
    // Laravel's nullable `count()` needs it to distinguish int, null and a
    // value that could be either.
    let first_arg_type = || {
        let arg = argument_list?.arguments.first()?;
        let resolved = resolve_rhs_expression(arg.value(), ctx);
        (!resolved.is_empty()).then(|| ResolvedType::types_joined(&resolved))
    };
    let fluent_count = crate::virtual_members::laravel::fluent_factory_count(
        &receiver_resolved,
        &method_name,
        arg_refs.first().copied(),
        &first_arg_type,
        &rctx,
    );

    let receiver_is_this = matches!(
        object,
        Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this"
    );
    let lsb_class = lsb_class_for_call(receiver_is_this, &receiver_resolved, ctx);

    let is_union = owner_classes.len() > 1;
    let mut union_results: Vec<ResolvedType> = Vec::new();
    // A receiver that is every one of its classes at once satisfies each of
    // their declarations, so what they return is intersected, not unioned.
    let receiver_is_intersection = is_union
        && !receiver_resolved.is_empty()
        && receiver_resolved
            .iter()
            .all(|rt| matches!(rt.type_string.kind(), TypeKind::Intersection(_)));
    let mut intersected_results: Vec<Vec<ResolvedType>> = Vec::new();

    for (idx, owner) in owner_classes.iter().enumerate() {
        // Build class-level template substitutions from the receiver's
        // generic type string (e.g. `Collection<int, User>` maps
        // `TKey => int, TValue => User`), merge in method-level
        // substitutions bound from the call's arguments, then override
        // with any `@psalm-if-this-is` inference from the receiver's
        // concrete type.
        let receiver_type = receiver_resolved
            .get(idx)
            .or_else(|| receiver_resolved.first())
            .map(|rt| &rt.type_string);
        let template_subs = crate::type_engine::call_resolution::build_call_template_subs(
            owner,
            &method_name,
            &arg_refs,
            receiver_type,
            &rctx,
        );

        // When the return type contains `static`/`self`/`$this` and the
        // receiver was resolved with generic parameters, use the
        // receiver's full type (e.g. `Builder<Article>`) for
        // substitution so the generics are preserved; otherwise fall
        // back to a plain FQN swap.
        let owner_key = owner.fqn();
        let receiver_intersection = receiver_intersection_for_owner(&receiver_resolved, &owner_key);
        let self_replace =
            |ty: &PhpType| match receiver_type_for_owner(&receiver_resolved, &owner_key) {
                Some(rt) => ty.replace_self_with_type(&rt),
                // An intersection receiver keeps every member: whichever
                // class late static binding lands on satisfies all of them,
                // not only the one that declared the method.
                None => match receiver_intersection {
                    Some(ref inter) => ty.replace_self_over_type(&owner_key, inter),
                    None => ty.replace_self_bound(&owner_key, lsb_class.as_deref()),
                },
            };

        let mut owner_results = resolve_owner_method_call(
            owner,
            &method_name,
            argument_list,
            ctx,
            is_static,
            &template_subs,
            &self_replace,
        );
        if let Some(count) = fluent_count {
            crate::virtual_members::laravel::carry_factory_count(
                &mut owner_results,
                &receiver_resolved,
                count,
            );
        }
        if !is_union {
            return owner_results;
        }
        if receiver_is_intersection {
            if !owner_results.is_empty() {
                intersected_results.push(owner_results);
            }
            continue;
        }
        ResolvedType::extend_unique(&mut union_results, owner_results);
    }
    if receiver_is_intersection {
        return intersect_owner_results(intersected_results);
    }

    // For intersection types, filter out `mixed` when concrete types exist.
    // When a receiver is an intersection like `IChild&IParent<C>`, each member
    // resolves the method independently: the unparameterized interface may
    // return `mixed` while the parameterized one returns `C`.  In an
    // intersection the most specific type wins, so discard `mixed` entries
    // when at least one non-mixed result is present.
    if union_results.len() > 1 {
        let has_non_mixed = union_results.iter().any(|rt| !rt.type_string.is_mixed());
        if has_non_mixed {
            union_results.retain(|rt| !rt.type_string.is_mixed());
        }
    }

    union_results
}

/// The member a selector names: an identifier as written, or a variable
/// holding one known string (`$name = 'length'; $obj->$name()`), which
/// calls that member exactly as spelling it out would.  `None` when only the
/// running code can tell (see `runtime_named_member_type`).
pub(super) fn selected_member_name(
    selector: &ClassLikeMemberSelector<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<String> {
    match selector {
        ClassLikeMemberSelector::Identifier(ident) => Some(bytes_to_str(ident.value).to_string()),
        ClassLikeMemberSelector::Variable(Variable::Direct(dv)) => {
            let resolved = resolve_var_types(bytes_to_str(dv.name), ctx, dv.span.start.offset);
            if resolved.is_empty() {
                return None;
            }
            match ResolvedType::types_joined(&resolved).kind() {
                TypeKind::Literal(literal) => match &**literal {
                    crate::php_type::LiteralValue::String(raw) => {
                        crate::util::unescape_php_string_literal(raw)
                    }
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// Combine what each member of an intersection receiver returns for the same
/// call into one intersection type.
///
/// Each entry of `per_owner` is one member's result.  A member returning
/// `mixed` adds nothing a sibling's concrete type does not already say.
/// When every remaining result is an object type the members are intersected
/// (`Foo&AnotherFoo`), with each resulting entry carrying the whole
/// intersection the way an intersection-typed variable does.  Anything else
/// (a scalar, a union, a nullable) is joined as a union.
fn intersect_owner_results(mut per_owner: Vec<Vec<ResolvedType>>) -> Vec<ResolvedType> {
    let is_mixed = |group: &[ResolvedType]| group.iter().all(|rt| rt.type_string.is_mixed());
    if per_owner.iter().any(|group| !is_mixed(group)) {
        per_owner.retain(|group| !is_mixed(group));
    }
    if per_owner.len() < 2 {
        return per_owner.pop().unwrap_or_default();
    }

    let mut members: Vec<PhpType> = Vec::new();
    let mut all_objects = true;
    for group in &per_owner {
        let joined = ResolvedType::types_joined(group);
        match joined.kind() {
            TypeKind::Intersection(parts) => {
                for part in parts.iter() {
                    if !members.contains(part) {
                        members.push(part.clone());
                    }
                }
            }
            TypeKind::Nullable(_) => {
                all_objects = false;
                break;
            }
            _ if joined.is_object_like() => {
                if !members.contains(&joined) {
                    members.push(joined);
                }
            }
            _ => {
                all_objects = false;
                break;
            }
        }
    }

    let intersection = (all_objects && members.len() > 1).then(|| PhpType::intersection(members));
    let mut results: Vec<ResolvedType> = Vec::new();
    for mut rt in per_owner.into_iter().flatten() {
        if let Some(ref intersection) = intersection {
            rt.type_string = intersection.clone();
        }
        ResolvedType::push_unique(&mut results, rt);
    }
    results
}

/// Expand union generic receiver types into separate owner entries.
///
/// When a variable has type `C<A>|C<B>`, the resolution pipeline produces
/// a single `ResolvedType` with `type_string = Union(Generic("C",[A]), Generic("C",[B]))`
/// and one `class_info` (the base class `C`).  Calling a method on such
/// a union should resolve each branch independently: `->get()` on
/// `C<A>|C<B>` where `get()` returns `T` should yield `A|B`.
///
/// This function detects such union-of-generics patterns and expands them
/// into separate owner classes, each with the appropriate template
/// substitutions applied.
pub(super) fn expand_union_generic_owners(
    owner_classes: Vec<Arc<ClassInfo>>,
    receiver_resolved: Vec<ResolvedType>,
    ctx: &VarResolutionCtx<'_>,
) -> (Vec<Arc<ClassInfo>>, Vec<ResolvedType>) {
    // Only expand when we have exactly one owner and the type_string
    // is a union with generic branches referencing the same base class.
    if owner_classes.len() != 1 || receiver_resolved.len() != 1 {
        return (owner_classes, receiver_resolved);
    }
    let rt = &receiver_resolved[0];
    let union_members = match &rt.type_string.kind() {
        TypeKind::Union(members) => members,
        _ => return (owner_classes, receiver_resolved),
    };

    // Check that at least two branches are generic types of the same
    // base class, and the class has template parameters.
    let base_cls = &owner_classes[0];
    if base_cls.template_params.is_empty() {
        return (owner_classes, receiver_resolved);
    }

    let base_fqn = base_cls.fqn();
    let base_short = base_cls.name.as_str();
    let is_same_base = |name: &str| -> bool {
        name == base_short
            || name == base_fqn.as_str()
            || crate::util::short_name(name) == base_short
    };
    let generic_branches: Vec<&PhpType> = union_members
        .iter()
        .filter(|m| matches!(m.kind(), TypeKind::Generic(g) if is_same_base(&g.name)))
        .collect();
    if generic_branches.len() < 2 {
        return (owner_classes, receiver_resolved);
    }

    // Expand: for each generic branch, apply the type args to produce
    // a substituted ClassInfo.
    let mut expanded_owners: Vec<Arc<ClassInfo>> = Vec::new();
    let mut expanded_resolved: Vec<ResolvedType> = Vec::new();

    for member in union_members {
        match member.kind() {
            TypeKind::Generic(g) if is_same_base(&g.name) => {
                let arc = crate::virtual_members::resolve_class_fully_with_type_args(
                    base_cls,
                    ctx.class_loader,
                    ctx.resolved_class_cache,
                    &g.args,
                );
                expanded_resolved.push(ResolvedType::from_both_arc(
                    member.clone(),
                    Arc::clone(&arc),
                ));
                expanded_owners.push(arc);
            }
            // Non-generic union members (e.g. scalars in `C<A>|int`)
            // are kept as type-string-only entries in receiver_resolved
            // but don't contribute an owner class.
            other => {
                expanded_resolved.push(ResolvedType::from_type_string(other.clone().into()));
            }
        }
    }

    (expanded_owners, expanded_resolved)
}

/// Find the receiver's type string that matches the given owner class name.
///
/// Scans `receiver_resolved` for a `ResolvedType` whose `class_info`
/// matches `owner_name` (short name or FQN) and whose `type_string` is a
/// `Generic` (i.e. carries generic parameters like `Builder<Article>`).
/// Returns the matching `PhpType` so that `replace_self_with_type` can
/// preserve those generic parameters when the method returns
/// `static`/`self`/`$this`.
///
/// Matching by short name alone is ambiguous for Laravel's dual
/// `Eloquent\Builder` / `Query\Builder` classes; FQN is preferred when
/// available so Query-mixin fluents like `lockForUpdate()` keep the
/// Eloquent receiver's `Builder<TModel>` type.
pub(super) fn receiver_type_for_owner(
    receiver_resolved: &[ResolvedType],
    owner_name: &str,
) -> Option<PhpType> {
    let owner_short = crate::util::short_name(owner_name);
    let mut short_match = None;
    for rt in receiver_resolved {
        let Some(ci) = rt.class_info.as_ref() else {
            continue;
        };
        if !matches!(rt.type_string.kind(), TypeKind::Generic(_)) {
            continue;
        }
        if ci.fqn().as_str() == owner_name || ci.name.as_str() == owner_name {
            return Some(rt.type_string.clone());
        }
        if short_match.is_none() && ci.name.as_str() == owner_short {
            short_match = Some(rt.type_string.clone());
        }
    }
    short_match
}

/// The receiver's intersection type string, when `owner_name` is one of the
/// classes it intersects.
///
/// A receiver typed `IfaceA&IfaceB` resolves to one entry per member, each
/// carrying the whole intersection as its type string (see
/// [`ResolvedType::tag_as_intersection`](crate::types::ResolvedType::tag_as_intersection)).
/// Calling a method `IfaceA` declares walks the members one at a time, and
/// without this the `static` it returns would come back as bare `IfaceA` —
/// dropping a half of the type the caller already proved.
pub(super) fn receiver_intersection_for_owner(
    receiver_resolved: &[ResolvedType],
    owner_name: &str,
) -> Option<PhpType> {
    let owner_short = crate::util::short_name(owner_name);
    receiver_resolved
        .iter()
        .find(|rt| {
            matches!(rt.type_string.kind(), TypeKind::Intersection(_))
                && rt.class_info.as_ref().is_some_and(|ci| {
                    ci.fqn().as_str() == owner_name || ci.name.as_str() == owner_short
                })
        })
        .map(|rt| rt.type_string.clone())
}

/// The class a `@return static` / `@return $this` annotation binds to for this
/// call, or `None` when the call fixes the class and the keyword collapses.
///
/// PHP only carries late static binding across a *forwarding* call: `$this->`,
/// `self::`, `static::`, and `parent::`.  Writing the class out — `A::create()`,
/// `(new A)->create()`, a variable declared `A` — pins it, so `static` there is
/// exactly `A` however `A` is subclassed.  PHPStan and Psalm both collapse
/// those, and keeping a bounded `static(A)` would claim an openness the call
/// does not have.
///
/// A receiver that is *itself* still bounded keeps the chain open, so
/// `$this->self()->self()` stays bound to the class the chain started from
/// rather than collapsing at the second hop.
///
/// A `$this` outside any class (a closure bound to a test case) forwards
/// from no class of its own, so the class it was resolved to fixes it.
fn lsb_class_for_call(
    receiver_is_this: bool,
    receiver_resolved: &[ResolvedType],
    ctx: &VarResolutionCtx<'_>,
) -> Option<crate::atom::Atom> {
    if receiver_is_this && !ctx.current_class.name.is_empty() {
        return Some(ctx.current_class.fqn());
    }
    receiver_resolved
        .iter()
        .find_map(|rt| match rt.type_string.kind() {
            TypeKind::StaticType(bound) | TypeKind::ThisType(bound) => Some(*bound),
            _ => None,
        })
}
