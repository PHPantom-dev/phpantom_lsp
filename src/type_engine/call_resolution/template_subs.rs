//! Template substitution for method-level `@template` parameters: builds
//! a substitution map from call-site argument texts, and unifies argument
//! types with the template parameters of the callable and generic
//! parameters they are passed to.

use std::collections::HashMap;

use crate::Backend;
use crate::atom::{Atom, AtomMap};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_type_compatible};
use crate::php_type::{PhpType, TypeKind};
use crate::types::*;

use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::variable::rhs_resolution::{
    TemplateBindingMode, classify_template_binding,
};

use super::constant_operands::constant_operand_subs;

impl Backend {
    /// Build a template substitution map for a method-level `@template` call.
    ///
    /// Finds the method on the class (or inherited), checks for template
    /// params and bindings, resolves argument types from the pre-split
    /// `arg_texts` slice using the call resolution context, and returns a
    /// `HashMap` mapping template parameter names to their resolved
    /// concrete types.
    ///
    /// Callers with an AST `ArgumentList` should extract per-argument text
    /// via [`extract_arg_texts_from_ast`] and convert to `&[&str]`.
    /// Callers with only raw text should use [`split_top_level_commas`](crate::text_scan::split_top_level_commas) first.
    ///
    /// Returns an empty map if the method has no template params, no
    /// bindings, or if argument types cannot be resolved.
    pub(crate) fn build_method_template_subs(
        class_info: &ClassInfo,
        method_name: &str,
        arg_texts: &[&str],
        ctx: &ResolutionCtx<'_>,
    ) -> HashMap<String, PhpType> {
        // Find the method — first on the class directly, then via inheritance.
        // An override without a docblock of its own inherits the ancestor's
        // `@template` tags in the merge, so a direct method that declares
        // none is looked up again there.
        let own = class_info.get_method(method_name);
        let may_inherit_templates = class_info.parent_class.is_some()
            || !class_info.interfaces.is_empty()
            || !class_info.used_traits.is_empty();
        let method = match own {
            Some(m) if !m.template_params().is_empty() || !may_inherit_templates => Some(m.clone()),
            _ => {
                let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
                    class_info,
                    ctx.file.class_loader,
                    ctx.resolved_class_cache,
                );
                merged
                    .get_method(method_name)
                    .cloned()
                    .or_else(|| own.cloned())
            }
        };

        let method = match method {
            Some(m) if !m.template_params().is_empty() => m,
            _ => return HashMap::new(),
        };

        let callee = super::TemplateCallee {
            parameters: &method.parameters,
            template_bindings: method.template_bindings(),
            template_param_bounds: method.template_param_bounds(),
        };
        let mut subs = super::bind_template_args(&callee, arg_texts, None, ctx);

        finish_template_subs(
            &mut subs,
            method.template_params(),
            method.template_param_bounds(),
            method.template_param_defaults(),
            method.return_type.as_ref(),
            ctx,
        );
        crate::virtual_members::laravel::patches::narrow_conditionable_value_binding(
            &method, &mut subs,
        );

        subs
    }
}

/// Build the full template substitution map for a method call: class-level
/// substitutions from the receiver's own generic arguments, method-level
/// substitutions bound from the call's arguments, and `@psalm-if-this-is`
/// substitutions inferred from the receiver's concrete type.
///
/// Shared by call-site return-type resolution
/// ([`crate::type_engine::variable::rhs_resolution::calls`]) and
/// `@psalm-this-out` receiver mutation (the forward walker) — both need
/// the same three-layer substitution map, just applied to different
/// target types (the method's return type vs. its self-out type).
pub(crate) fn build_call_template_subs(
    owner: &ClassInfo,
    method_name: &str,
    arg_texts: &[&str],
    receiver_type: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let class_level_subs: HashMap<String, PhpType> = receiver_type
        .map(|ty| {
            if ty.is_self_like()
                || matches!(ty.kind(), TypeKind::Generic(g) if g.args.iter().any(|a| a.is_self_like()))
            {
                return HashMap::new();
            }

            let mut values: HashMap<String, PhpType> = owner
                .template_param_defaults
                .iter()
                .map(|(name, default)| (name.to_string(), default.clone()))
                .collect();
            if let TypeKind::Generic(g) = ty.kind()
                && !g.args.is_empty()
                && !owner.template_params.is_empty()
            {
                values.extend(
                    owner
                        .template_params
                        .iter()
                        .zip(g.args.iter())
                        .map(|(name, ty)| (name.to_string(), ty.clone())),
                );
            }
            // A receiver typed without some of the owner's arguments (a bare
            // `Iterator` hint) has them erased, as `type_hint_to_classes_typed`
            // erases them for the class itself.  Inside the owner's own body
            // its parameters are still in scope and stay unbound.
            let inside_owner = ctx
                .current_class
                .is_some_and(|cc| cc.fqn().eq_ignore_ascii_case(&owner.fqn()));
            if !inside_owner && values.len() < owner.template_params.len() {
                for (name, arg) in owner
                    .template_params
                    .iter()
                    .zip(crate::inheritance::default_type_args(owner))
                {
                    values.entry(name.to_string()).or_insert(arg);
                }
            }
            values
        })
        .unwrap_or_default();

    let method_template_subs =
        Backend::build_method_template_subs(owner, method_name, arg_texts, ctx);

    let if_this_is_subs: HashMap<String, PhpType> = owner
        .get_method_ci(method_name)
        .and_then(|m| m.if_this_is())
        .and_then(|pattern| {
            let method = owner.get_method_ci(method_name)?;
            Some(super::if_this_is::infer_if_this_is_subs(
                pattern,
                receiver_type?,
                method.template_params(),
                method.template_param_bounds(),
            ))
        })
        .unwrap_or_default();

    let mut template_subs = class_level_subs;
    template_subs.extend(method_template_subs);
    template_subs.extend(if_this_is_subs);
    template_subs
}

/// Parameter names (`$`-prefixed) that are the *exclusive* binding site
/// for a `@template` parameter at this call site.
///
/// A template bound from exactly one argument has no independent
/// signal to check that argument against: the substituted type came from
/// resolving that same argument, so any diagnostic that compares the
/// argument to it again is comparing the argument against itself through
/// two potentially-diverging resolution paths. For example, PHPUnit's
/// `assertSame(ExpectedType $expected, mixed $actual)` binds
/// `ExpectedType` only from `$expected`, so checking `$expected` against
/// `ExpectedType` is circular and can never legitimately fail.
///
/// What counts is the binding sites the *caller filled*, not the ones the
/// signature declares. Laravel's `travelTo` names `TDate` in both `$date`
/// and the optional `$callback`'s callable signature; a call that passes
/// only a date still binds `TDate` from that one argument, so checking it
/// is just as circular as if `$callback` did not exist. Omitted
/// parameters are dropped here for that reason.
///
/// A template two arguments both bind is not covered: those disagree with
/// each other rather than with themselves, which is a real check.
///
/// The substitution is circular, but a template's `of` bound is not: it
/// is what the argument had to satisfy to bind the template at all. Each
/// parameter maps to its declared type with the templates only it binds
/// replaced by their bounds, or to `None` when none of those templates
/// declares one, since then there is nothing to check. Every other
/// template the declaration binds becomes `mixed`: another argument
/// decides it, and that argument's own check is where the two can
/// disagree.
///
/// The bound only stands in for the binding where the template is the
/// value handed over. A template bound from a callable's parameter list
/// (`@param callable(T): string`) is what the callback accepts, and a
/// callback accepting less than the whole bound is the point: `fn(User $u)`
/// binds `T` to `User`. There the binding, from `call_subs`, is checked in
/// place of the bound once it is known to satisfy it.
pub(crate) fn self_bound_template_params(
    bindings: &[(Atom, Atom)],
    parameters: &[ParameterInfo],
    arg_texts: &[&str],
    bound_of: &dyn Fn(&Atom) -> Option<PhpType>,
    call_subs: &HashMap<String, PhpType>,
    class_loader: &ResolvedClassLoader<'_>,
) -> AtomMap<Option<PhpType>> {
    let self_bound = exclusively_bound_templates(bindings, parameters, arg_texts);
    let mut result = AtomMap::default();
    for (tpl_name, param_name) in &self_bound {
        let bounds = result.entry(*param_name).or_insert_with(HashMap::new);
        let Some(bound) = bound_of(tpl_name) else {
            continue;
        };
        let hint = parameters
            .iter()
            .find(|p| p.name == param_name.as_str())
            .and_then(|p| p.type_hint.as_ref());
        let accepted_by_callback = matches!(
            classify_template_binding(tpl_name, hint),
            TemplateBindingMode::CallableParamType(_)
        );
        let checked = call_subs
            .get(tpl_name.as_str())
            .filter(|binding| {
                accepted_by_callback && is_type_compatible(binding, &bound, class_loader, false)
            })
            .cloned()
            .unwrap_or(bound);
        bounds.insert(tpl_name.to_string(), checked);
    }
    result
        .into_iter()
        .map(|(param_name, bounds): (Atom, HashMap<String, PhpType>)| {
            if bounds.is_empty() {
                return (param_name, None);
            }
            let declared = parameters
                .iter()
                .find(|p| p.name == param_name.as_str())
                .and_then(|p| p.type_hint.as_ref());
            let checked = declared.map(|hint| {
                let mut bound_subs: HashMap<String, PhpType> = bindings
                    .iter()
                    .map(|(tpl_name, _)| (tpl_name.to_string(), PhpType::mixed()))
                    .collect();
                bound_subs.extend(bounds);
                hint.substitute(&bound_subs)
            });
            (param_name, checked)
        })
        .collect()
}

/// The `(template, parameter)` bindings the call fills, keeping only the
/// templates exactly one filled parameter binds.
fn exclusively_bound_templates(
    bindings: &[(Atom, Atom)],
    parameters: &[ParameterInfo],
    arg_texts: &[&str],
) -> Vec<(Atom, Atom)> {
    let bound = crate::call_args::bind_text_args_to_params(parameters, arg_texts);
    let was_passed = |param_name: &Atom| {
        parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
            .is_some_and(|idx| bound.get(idx).is_some_and(Option::is_some))
    };
    let filled: Vec<&(Atom, Atom)> = bindings
        .iter()
        .filter(|(_, param_name)| was_passed(param_name))
        .collect();

    filled
        .iter()
        .filter(|(tpl_name, _)| filled.iter().filter(|(t, _)| t == tpl_name).count() == 1)
        .map(|binding| **binding)
        .collect()
}

/// Recover a template parameter no argument names directly, from the
/// *bound* of a template that an argument did bind.
///
/// `usort`'s stub declares `@template T` and `@template TArray of array<T>`
/// with `@param TArray $array` / `@param callable(T, T): int $callback`.
/// Nothing at the call site binds `T`: the array argument binds `TArray`,
/// and the callback is the very thing whose parameters `T` is meant to
/// type. Unifying `TArray`'s bound (`array<T>`) against what `TArray` was
/// bound to (`list<Error>`) recovers `T = Error`, which is what turns an
/// untyped `usort($errors, fn ($a, $b) => …)` callback into a typed one.
///
/// The bound may also name a *supertype* of what the other template was
/// bound to rather than its own shape. `CollectedDataNode::get()` declares
/// `@template TCollector of Collector<Node, TValue>` with
/// `@param class-string<TCollector>`, so `TValue` is whatever the collector
/// class's own `@implements Collector<…, …>` says it is — read off the
/// class's ancestry rather than off the argument's shape.
///
/// Runs before the fill-in below, so it only ever reads bindings that came
/// from real arguments, never a bound standing in for a missing one.
fn propagate_bound_template_bindings(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &crate::atom::AtomMap<PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    for tpl_name in template_params {
        if subs.contains_key(tpl_name.as_str()) {
            continue;
        }
        // Iterate the declared order rather than the bounds map so the
        // binding a template picks up does not depend on hash order.
        for other in template_params {
            if other == tpl_name {
                continue;
            }
            let Some(bound) = template_param_bounds.get(other) else {
                continue;
            };
            let Some(bound_to) = subs.get(other.as_str()) else {
                continue;
            };
            if let Some(recovered) = unify_template(bound, bound_to, tpl_name)
                .or_else(|| ancestor_bound_binding(bound, bound_to, tpl_name, ctx))
            {
                subs.insert(tpl_name.to_string(), recovered);
                break;
            }
        }
    }
}

/// Read `tpl_name` off the ancestry of what another template was bound to,
/// for a bound that names a generic supertype (`@template TCollector of
/// Collector<Node, TValue>`).
///
/// A `class-string<TCollector>` parameter binds the class itself, so the
/// generic arguments live on its `@extends`/`@implements` clauses, not on
/// the bound type it was matched against.
fn ancestor_bound_binding(
    bound: &PhpType,
    bound_to: &PhpType,
    tpl_name: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let TypeKind::Generic(g) = bound.kind() else {
        return None;
    };
    let position = g.args.iter().position(|a| a.is_named(tpl_name))?;
    let subject = bound_to.unwrap_class_string_inner().unwrap_or(bound_to);
    crate::inheritance::extract_generic_arg_from_ancestor(
        subject,
        &g.name,
        position,
        ctx.file.class_loader,
    )
}

/// Finish a template substitution map: bind the constants its types read
/// through a type operator, recover the templates only another template's
/// bound names, then fill in the template params no argument bound.
///
/// The halves exist so a raw name never leaks downstream. An unbound
/// template resolves to its declared default (`@template T of object =
/// \stdClass` → `stdClass`), else its upper bound (`@template T of Foo` →
/// `Foo`) or `mixed`, following PHPStan's `resolveToBounds()`. A constant
/// operand resolves to the array shape it names, which is what lets the
/// substitution every call site already runs finish the operator:
/// `key-of<TABLE>` becomes the table's own keys, and `TABLE[K]` picks out
/// the single value the argument bound `K` to.
///
/// The constant bindings are applied to the bounds as well, so a call that
/// leaves `K` unbound still reads `TABLE[key-of<TABLE>]` as the table's
/// value union rather than giving up on the operator.
pub(crate) fn finish_template_subs(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &crate::atom::AtomMap<PhpType>,
    template_param_defaults: &[(Atom, PhpType)],
    return_type: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    drop_bindings_outside_bounds(subs, template_params, template_param_bounds, ctx);
    propagate_bound_template_bindings(subs, template_params, template_param_bounds, ctx);

    let constant_subs = constant_operand_subs(
        return_type
            .into_iter()
            .chain(template_param_bounds.values()),
        template_params,
        ctx,
    );

    for tpl_name in template_params {
        if subs.contains_key(tpl_name.as_str()) {
            continue;
        }
        let fallback = template_param_defaults
            .iter()
            .find(|(name, _)| name == tpl_name)
            .map(|(_, default)| default.substitute(subs))
            .or_else(|| {
                template_param_bounds
                    .get(tpl_name)
                    .map(|bound| bound.substitute(&constant_subs))
            })
            .unwrap_or_else(PhpType::mixed);
        subs.insert(tpl_name.to_string(), fallback);
    }

    for (name, shape) in constant_subs {
        subs.entry(name).or_insert(shape);
    }
}

/// Unbind every template an argument bound to a class its bound rules out.
///
/// `@template F of User` handed an `Article` cannot be `Article`: the call
/// is an error, and the template falls back to the bound it fails to
/// satisfy, which is what the call is declared to deal in (PHPStan does the
/// same).  Only a class judged against a class bound is decided here; a
/// class nobody can load, a bound that still names a template, or anything
/// but a class on either side keeps its binding.
fn drop_bindings_outside_bounds(
    subs: &mut HashMap<String, PhpType>,
    template_params: &[Atom],
    template_param_bounds: &AtomMap<PhpType>,
    ctx: &ResolutionCtx<'_>,
) {
    if template_param_bounds.is_empty() || subs.is_empty() {
        return;
    }
    let loaded_class_names = |ty: &PhpType| -> Option<Vec<Atom>> {
        let mut names = Vec::new();
        for member in ty.union_members() {
            let name = match member.kind() {
                TypeKind::Named(name) if !crate::php_type::is_keyword_type(name) => *name,
                TypeKind::Generic(g) if !crate::php_type::is_keyword_type(&g.name) => g.name,
                _ => return None,
            };
            if template_params.contains(&name) || (ctx.file.class_loader)(Fqn::new(&name)).is_none()
            {
                return None;
            }
            names.push(name);
        }
        Some(names)
    };
    for tpl_name in template_params {
        let Some(bound) = template_param_bounds.get(tpl_name) else {
            continue;
        };
        let Some(bound_to) = subs.get(tpl_name.as_str()) else {
            continue;
        };
        let (Some(bound_classes), Some(bound_to_classes)) =
            (loaded_class_names(bound), loaded_class_names(bound_to))
        else {
            continue;
        };
        let satisfies = bound_to_classes.iter().all(|sub| {
            bound_classes.iter().any(|sup| {
                crate::class_lookup::is_subtype_of_names(sub, sup, ctx.file.class_loader)
            })
        });
        if !satisfies {
            subs.remove(tpl_name.as_str());
        }
    }
}

/// `bindings` in the order they should be bound: those read off a
/// callable-typed parameter come after all the rest.
///
/// A callable argument's return type is inferred with its parameters
/// seeded from the templates the other arguments bound, and a template it
/// names in a parameter position only binds when nothing else did, so both
/// need every other argument bound first, whatever order the `@param` tags
/// are written in.
pub(crate) fn callable_bindings_last<'a>(
    bindings: &'a [(Atom, Atom)],
    params: &'a [ParameterInfo],
) -> impl Iterator<Item = &'a (Atom, Atom)> {
    let is_callable = move |binding: &&(Atom, Atom)| {
        params
            .iter()
            .find(|p| p.name == binding.1.as_str())
            .and_then(|p| p.type_hint.as_ref())
            .is_some_and(|h| h.callable_param_types().is_some())
    };
    bindings
        .iter()
        .filter(move |b| !is_callable(b))
        .chain(bindings.iter().filter(is_callable))
}

/// Bind a template parameter that a `@param Closure(T): void` hint names in
/// the callback's *parameter* list, reading the type off the closure
/// argument's own annotation at `position`.
///
/// The annotation is raw source text, so it carries the file's spelling of
/// the class rather than its FQCN (`Support\Pen` behind a `use App\Support;`).
/// A template bound from it is compared against types that arrived fully
/// qualified, so the spelling is canonicalised here.
///
/// A parameter position is contravariant: the closure accepting a
/// `Timeline` says nothing about the `Timeline<Percentage>` the call hands
/// it.  Callers therefore only bind from here when no other argument bound
/// the template (see [`callable_bindings_last`]).
pub(crate) fn bind_callable_param_template(
    arg_text: &str,
    position: usize,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let param_type = crate::completion::source::helpers::extract_closure_param_type_from_text(
        arg_text, position,
    )?;
    Some(ctx.written_names().resolve_type(&param_type))
}

/// Bind the template a `@param callable(...): …` hint names in its return
/// type, from the closure argument written at the call site.
///
/// A closure's return type is what its body produces, narrowed by what it
/// declares (PHPStan's `intersectButNotNever`).  `fn (Timeline $t): Timeline
/// => $t` handed a `Timeline<Percentage>` returns `Timeline<Percentage>`,
/// and `fn ($c): array => arrStr($c)` returns the `array<int, string>` that
/// `arrStr()` declares.  The body is resolved with each closure parameter
/// seeded from the hint's own parameter types wherever the templates they
/// name are already in `bound`, so an untyped `fn ($t) => $t` receives what
/// the call hands it.  An annotation the body cannot narrow (a scalar, or a
/// body that resolves to something unrelated) stands as written, and an
/// unannotated closure falls back to its generator yields.
///
/// The template is rarely the whole return type: `array<TKey, TValue>`,
/// `list<TValue>`, and Laravel's `Collection<TKey, TValue>|array<TKey,
/// TValue>` each name it at a position inside a larger shape.  The inferred
/// type is matched against that shape so each template binds to its own
/// part rather than to the whole return type.
///
/// `bindings` is the callee's full template binding list, which is how an
/// unbound template in the hint's parameter types is told apart from a
/// class name.
pub(crate) fn bind_callable_return_template(
    arg_text: &str,
    param_hint: Option<&PhpType>,
    tpl_name: &str,
    bound: &HashMap<String, PhpType>,
    bindings: &[(Atom, Atom)],
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let declared_ret = param_hint.and_then(|h| h.callable_return_type());
    let ret_type = if crate::completion::source::helpers::is_closure_like_text(arg_text.trim()) {
        let seeds = callable_param_seeds(param_hint, bound, bindings);
        Backend::infer_closure_return_type_seeded(arg_text, &seeds, ctx)?
    } else {
        // A callable held in a variable or returned by a call has no body
        // to read, but its own type (`callable(callable(): int): string`)
        // still says what it returns.
        Backend::resolve_arg_text_to_type(arg_text, ctx)?
            .callable_return_type()?
            .clone()
    };
    // A `null` the declared return takes in its own right (`Closure(): ?T`)
    // says nothing about `T`.
    if ret_type.is_null()
        && declared_ret.is_some_and(|declared| {
            matches!(declared.kind(), TypeKind::Nullable(_))
                || declared.union_members().iter().any(|m| m.is_null())
        })
    {
        return None;
    }
    let Some(declared) = declared_ret else {
        return Some(ret_type);
    };
    if let Some(bound) = unify_template(declared, &ret_type, tpl_name) {
        return Some(bound);
    }
    // Only a return declared as the template itself is the whole value;
    // one that wraps it (`Collection<TKey, TValue>`) handed a return the
    // wrapper's arguments cannot be read off (a bare `Collection`) binds
    // nothing, rather than the whole return at every position.
    declared
        .union_members()
        .iter()
        .any(|m| m.is_named(tpl_name))
        .then_some(ret_type)
}

/// The types a callable hint promises each of its parameters, with the
/// callee's already-bound templates substituted in.
///
/// A position whose hint still names an unbound template has nothing
/// concrete to promise and is left `None`.
fn callable_param_seeds(
    param_hint: Option<&PhpType>,
    bound: &HashMap<String, PhpType>,
    bindings: &[(Atom, Atom)],
) -> Vec<Option<PhpType>> {
    let Some(params) = param_hint.and_then(|h| h.callable_param_types()) else {
        return Vec::new();
    };
    params
        .iter()
        .map(|p| {
            let names_unbound = bindings.iter().any(|(t, _)| {
                !bound.contains_key(t.as_str())
                    && crate::type_engine::variable::rhs_resolution::type_contains_name(
                        &p.type_hint,
                        t,
                    )
            });
            (!names_unbound).then(|| p.type_hint.substitute(bound))
        })
        .collect()
}

/// Bind a template parameter by walking a parameter hint and an argument
/// type together.
///
/// Returns the argument's subtree at whichever position `tpl_name` occupies
/// in `param_hint`.  For `@param array<string, array<string, T>> $in` and an
/// argument typed `array<string, array<string, Leaf>>`, that is `Leaf` —
/// where positional extraction, which unwraps a single level, would bind the
/// whole inner array.
///
/// A union hint offers several shapes and the argument picks one: for
/// `@param iterable<array-key, T>|T $value` (Laravel's `Collection::wrap()`)
/// an `array<string>` argument binds `T` to `string` through the iterable
/// alternative, while a `string` argument binds `T` to `string` through the
/// bare one.  The bare alternative matches anything, so it is only used when
/// no other alternative fits.
///
/// Returns `None` when the hint does not name the template, or when the two
/// shapes disagree, leaving the caller's positional extraction to run.
pub(super) fn unify_template(
    param_hint: &PhpType,
    arg_type: &PhpType,
    tpl_name: &str,
) -> Option<PhpType> {
    match param_hint.kind() {
        TypeKind::Named(name) if &**name == tpl_name => Some(arg_type.clone()),
        TypeKind::Union(members) => {
            let mut has_bare = false;
            for member in members {
                if member.is_null() {
                    continue;
                }
                if member.is_named(tpl_name) {
                    has_bare = true;
                    continue;
                }
                if let Some(unified) = unify_template(member, arg_type, tpl_name) {
                    return Some(unified);
                }
            }
            has_bare.then(|| bare_template_remainder(members, arg_type, tpl_name))
        }
        TypeKind::Generic(hint) => {
            // A shape spells its entries out one by one; against a container
            // hint it stands for the container it describes, whose key and
            // value are what its entries have in common.
            let generalized;
            let arg_type = if !arg_type.is_empty_array_shape()
                && crate::type_engine::variable::rhs_resolution::is_array_like_wrapper(&hint.name)
            {
                generalized = arg_type.generalized_array();
                &generalized
            } else {
                arg_type
            };
            if let TypeKind::Generic(arg) = arg_type.kind()
                && arg.args.len() == hint.args.len()
            {
                return hint
                    .args
                    .iter()
                    .zip(arg.args.iter())
                    .find_map(|(h, a)| unify_template(h, a, tpl_name));
            }
            // Two container types whose arguments don't line up positionally
            // (`iterable<TKey, TValue>` against `list<string>`) still line up
            // key-to-key and value-to-value.
            if !crate::type_engine::variable::rhs_resolution::is_array_like_wrapper(&hint.name) {
                return None;
            }
            // An empty array has no key or value for the template to be.
            if arg_type.is_empty_array_shape() && names_template_directly(param_hint, tpl_name) {
                return Some(PhpType::never());
            }
            let key_match = (hint.args.len() >= 2)
                .then(|| arg_type.extract_key_type(false))
                .flatten()
                .and_then(|k| unify_template(&hint.args[0], k, tpl_name));
            key_match.or_else(|| {
                let value_hint = hint.args.last()?;
                // An untyped `array` still says "the argument is a container",
                // its elements are just unknown — `mixed`.  Without this the
                // hint does not match at all and a bare `T` alternative in a
                // union hint binds the array itself as the element type.
                let mixed = PhpType::mixed();
                let value = match arg_type.extract_value_type(false) {
                    Some(v) => v,
                    None if arg_type.is_bare_array() => &mixed,
                    None => return None,
                };
                unify_template(value_hint, value, tpl_name)
            })
        }
        TypeKind::Array(inner) => match arg_type.kind() {
            TypeKind::Array(arg_inner) => unify_template(inner, arg_inner, tpl_name),
            // `T[]` against `array<K, V>` / `list<V>`: the value type lines up.
            _ => arg_type
                .extract_value_type(false)
                .and_then(|v| unify_template(inner, v, tpl_name)),
        },
        // `array{k: T}` against a shape: `T` is what the argument holds under
        // the same key.
        TypeKind::ArrayShape(entries) => {
            let mut position = 0usize;
            entries.iter().find_map(|entry| {
                let value = match &entry.key {
                    Some(key) => arg_type.shape_value_type(key),
                    None => {
                        position += 1;
                        arg_type.shape_value_type(&(position - 1).to_string())
                    }
                }?;
                unify_template(&entry.value_type, value, tpl_name)
            })
        }
        TypeKind::Nullable(inner) => unify_template(inner, arg_type.unwrap_nullable(), tpl_name),
        _ => None,
    }
}

/// What a bare `T` alternative of a union hint binds to: the part of the
/// argument the hint's other alternatives don't already account for.
///
/// For `@param Tvalue|string|null` and a `string|float|bool|null` argument
/// that is `float|bool`, so the return type's `Tvalue` does not bring the
/// `null` back.  When the other alternatives cover the whole argument, the
/// argument itself is the binding.
fn bare_template_remainder(
    hint_members: &[PhpType],
    arg_type: &PhpType,
    tpl_name: &str,
) -> PhpType {
    let others: Vec<&PhpType> = hint_members
        .iter()
        .filter(|m| !crate::type_engine::variable::rhs_resolution::type_contains_name(m, tpl_name))
        .collect();
    if others.is_empty() {
        return arg_type.clone();
    }
    let arg_members = arg_type.union_members();
    let remainder: Vec<PhpType> = arg_members
        .iter()
        .filter(|a| !others.iter().any(|o| a.is_subtype_of(o)))
        .map(|a| (*a).clone())
        .collect();
    if remainder.is_empty() || remainder.len() == arg_members.len() {
        arg_type.clone()
    } else {
        PhpType::union(remainder)
    }
}

/// How many arguments the `wrapper_name<…>` in a parameter hint takes, or 1
/// when the hint holds no such generic.
///
/// The wrapper can sit inside a union (`ArrayIterator`'s constructor takes
/// `array<TKey, TValue>|object`), and reading the arity off the union
/// itself would count one argument and bind the key template to the value
/// type.
pub(super) fn wrapper_arity(param_hint: Option<&PhpType>, wrapper_name: &str) -> usize {
    let wrapper_short = crate::util::short_name(wrapper_name);
    let find = |ty: &PhpType| -> Option<usize> {
        let members: &[PhpType] = match ty.kind() {
            TypeKind::Union(members) => members,
            _ => std::slice::from_ref(ty),
        };
        members
            .iter()
            .find_map(|m| match m.unwrap_nullable().kind() {
                TypeKind::Generic(g)
                    if crate::util::short_name(&g.name).eq_ignore_ascii_case(wrapper_short) =>
                {
                    Some(g.args.len())
                }
                _ => None,
            })
    };
    match param_hint {
        Some(hint) => find(hint).unwrap_or(match hint.kind() {
            TypeKind::Generic(g) => g.args.len(),
            _ => 1,
        }),
        None => 1,
    }
}

/// Whether a generic hint names `tpl_name` as one of its own arguments.
///
/// The flat case (`array<TKey, TValue>`) is the positional extractor's
/// business — it knows the key/value arity quirks — so structural
/// unification stays out of its way.
pub(super) fn names_template_directly(hint: &PhpType, tpl_name: &str) -> bool {
    matches!(hint.kind(), TypeKind::Generic(g)
        if g.args.iter().any(|a| matches!(a.kind(), TypeKind::Named(n) if &**n == tpl_name)))
}

/// Generalize a literal bound to a template of an object type.
///
/// An object outlives the call that shaped it, so `new Box(42)` is a
/// `Box<int>` that can hold any int later rather than a `Box<42>`, the way
/// PHPStan generalizes it; a `@phpstan-self-out self<T>` result is the same
/// kind of type. A bound that is itself a scalar (`@template T of 'a'|'b'`,
/// `of int`) says the literal is the point, except `array-key`, which only
/// says the value can index an array.
pub(crate) fn generalize_object_template_arg(ty: &PhpType, bound: Option<&PhpType>) -> PhpType {
    let keeps_literals = bound.is_some_and(|bound| {
        !bound.is_array_key()
            && bound.union_members().iter().all(|member| {
                member.is_string_subtype()
                    || member.is_int_subtype()
                    || member.is_float_subtype()
                    || member.is_bool()
                    || member.is_true()
                    || member.is_false()
                    || matches!(member.kind(), TypeKind::Named(n)
                        if n.eq_ignore_ascii_case("scalar") || n.eq_ignore_ascii_case("numeric"))
            })
    });
    if keeps_literals {
        ty.clone()
    } else {
        ty.widen_scalar_literals()
    }
}

#[cfg(test)]
mod class_template_sub_tests {
    use std::sync::Arc;

    use super::build_call_template_subs;
    use crate::atom::atom;
    use crate::class_lookup::Fqn;
    use crate::php_type::PhpType;
    use crate::type_engine::resolver::{FileCtx, ResolutionCtx};
    use crate::types::ClassInfo;

    #[test]
    fn unresolved_self_like_receivers_do_not_apply_class_defaults() {
        let owner = ClassInfo {
            name: atom("PendingRequest"),
            template_params: vec![atom("TAsync")],
            template_param_defaults: [(atom("TAsync"), PhpType::parse("false"))]
                .into_iter()
                .collect(),
            ..ClassInfo::default()
        };
        let classes = Vec::new();
        let class_loader = |_: Fqn<'_>| -> Option<Arc<ClassInfo>> { None };
        let ctx = ResolutionCtx::new(
            FileCtx::new(
                "",
                &classes,
                &class_loader,
                crate::test_fixtures::fqn_source_loader(&class_loader),
            ),
            None,
            0,
        );

        assert!(
            build_call_template_subs(&owner, "send", &[], Some(&PhpType::parse("static")), &ctx,)
                .is_empty()
        );
        assert!(
            build_call_template_subs(
                &owner,
                "send",
                &[],
                Some(&PhpType::parse("PendingRequest<static>")),
                &ctx,
            )
            .is_empty()
        );
    }
}
