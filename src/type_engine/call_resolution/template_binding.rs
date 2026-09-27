/// Binding a callee's `@template` parameters from the arguments written at
/// a call site.
///
/// Functions, methods, and constructors all bind the same way: each
/// `@param` that names a template is matched against the argument PHP
/// routes to it, and what the argument resolves to is read off at the
/// position the hint names the template. What differs is only what
/// happens afterwards (filling in the templates nothing bound, or
/// generalizing a class template's literals), which the entry points
/// here and their callers take care of.
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::atom::{Atom, AtomMap, atom};
use crate::php_type::{PhpType, TypeKind};
use crate::type_engine::resolver::ResolutionCtx;
use crate::type_engine::variable::rhs_resolution::{
    ArgWalkerTypes, TemplateBindingMode, array_element_binding, bound_binding_hint,
    candidate_binding_modes, class_string_generic_binding, class_string_inner_binding,
    classify_template_binding, extract_array_position, extract_generic_args_from_ancestor,
    insert_or_union, is_array_like_wrapper, resolve_arg_call_raw_type,
    resolve_arg_iterable_raw_type, resolve_array_literal_generic,
};
use crate::types::{ClassInfo, MethodInfo, ParameterInfo};

use super::return_types::literal_arg_type;
use super::template_subs::{
    array_literal_element_type, array_literal_shape_type, bind_callable_param_template,
    bind_callable_return_template, callable_bindings_last, generalize_object_template_arg,
    names_template_directly, type_operator_bound_literal, unify_template, wrapper_arity,
};

/// The parts of a callee's signature its templates bind through.
pub(crate) struct TemplateCallee<'a> {
    pub parameters: &'a [ParameterInfo],
    /// `(template, parameter)` pairs, one per `@param` naming a template.
    pub template_bindings: &'a [(Atom, Atom)],
    /// The bounds of the templates the parameters name.  For a
    /// constructor these are the class's, which is where a class
    /// template's `of …` lives.
    pub template_param_bounds: &'a AtomMap<PhpType>,
}

/// Bind each of `callee`'s templates from the call-site `arg_texts`,
/// leaving the templates no argument binds out of the map.
///
/// Every binding site unions into the substitution rather than
/// overwriting it, so a template bound from several parameters resolves
/// to what all of its arguments have in common: `@param T[] $a, T[] $b`
/// with `combine([1], ['x'])` binds `T` to `int|string`.  Letting the
/// last binding site win would leave every other argument measured
/// against a type taken from one of its siblings.
///
/// `walker_types` answers for an argument the text-driven resolver
/// cannot read, when the caller reached the call through the AST.
pub(crate) fn bind_template_args(
    callee: &TemplateCallee<'_>,
    arg_texts: &[&str],
    walker_types: Option<ArgWalkerTypes<'_>>,
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut subs = HashMap::new();

    // Bind the raw source-order argument texts to parameters by PHP's rules
    // so a named argument (`id: Foo::class`) is routed to the parameter it
    // targets rather than its ordinal slot, and its `name:` prefix is
    // stripped off the value.
    let bound = crate::call_args::bind_text_args_to_params(callee.parameters, arg_texts);

    let param_index = |param_name: &Atom| {
        callee
            .parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
    };
    let is_written = |(_, param_name): &&(Atom, Atom)| {
        param_index(param_name).is_some_and(|idx| matches!(bound.get(idx), Some(Some(_))))
    };
    // A default only stands in for an argument, so the parameters left out
    // bind after every argument that was written.
    let ordered = callable_bindings_last(callee.template_bindings, callee.parameters);
    let (written, omitted): (Vec<&(Atom, Atom)>, Vec<_>) = ordered.partition(is_written);

    for (tpl_name, param_name) in written.into_iter().chain(omitted) {
        let Some(param_idx) = param_index(param_name) else {
            continue;
        };
        let param = &callee.parameters[param_idx];

        let via_bound = bound_binding_hint(
            tpl_name,
            param.type_hint.as_ref(),
            callee.template_param_bounds,
        );
        let param_hint = via_bound.as_ref().or(param.type_hint.as_ref());
        let tpl_bound = callee.template_param_bounds.get(&atom(tpl_name));

        let arg_text = match bound.get(param_idx).and_then(|o| o.as_deref()) {
            Some(text) => text,
            None => match omitted_arg_default(
                param.default_value.as_deref(),
                tpl_name,
                tpl_bound,
                param_hint,
                &subs,
            ) {
                Some(default) => default,
                None => continue,
            },
        };

        if let Some(literal) = type_operator_bound_literal(tpl_bound, arg_text) {
            insert_or_union(&mut subs, tpl_name.to_string(), literal);
            continue;
        }

        // A union `@param` names one binding site per alternative
        // (`Collection<TKey, TValue>|array<TKey, TValue>`), and the one the
        // argument's own shape matches is the one that should bind. Try
        // them in order and stop at the first that resolves; a non-union
        // hint yields a single mode, which is the path every other
        // parameter takes.
        for mode in candidate_binding_modes(tpl_name, param_hint) {
            if apply_binding_mode(
                &mut subs,
                &mode,
                tpl_name,
                arg_text,
                param_hint,
                callee.template_bindings,
                walker_types,
                ctx,
            ) {
                break;
            }
        }
    }

    subs
}

/// Bind a constructor's templates from `new` arguments, the class's own
/// templates included.
///
/// An object outlives the call that shaped it, so a literal argument is
/// generalized to its base type unless the template's bound says the
/// literal is the point (see [`generalize_object_template_arg`]).
fn bind_constructor_template_args(
    class: &ClassInfo,
    ctor: &MethodInfo,
    arg_texts: &[&str],
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    // A constructor rarely declares templates of its own, but when it does
    // their bounds sit beside the class's.
    let bounds = if ctor.template_param_bounds.is_empty() {
        Cow::Borrowed(&class.template_param_bounds)
    } else {
        let mut merged = ctor.template_param_bounds.clone();
        merged.extend(
            class
                .template_param_bounds
                .iter()
                .map(|(name, bound)| (*name, bound.clone())),
        );
        Cow::Owned(merged)
    };
    let callee = TemplateCallee {
        parameters: &ctor.parameters,
        template_bindings: &ctor.template_bindings,
        template_param_bounds: &bounds,
    };
    let mut subs = bind_template_args(&callee, arg_texts, None, ctx);
    for (name, ty) in subs.iter_mut() {
        *ty = generalize_object_template_arg(ty, bounds.get(&atom(name)));
    }
    subs
}

/// The default an omitted argument binds its template through, if any.
///
/// A default is only a binding where it says something the call site
/// would otherwise have said: a literal a type-operator bound resolves
/// against, a `Foo::class` naming the class, or a `null` that nothing
/// else in the hint takes.  An argument that already bound the template
/// wins over any default.
fn omitted_arg_default<'d>(
    default_value: Option<&'d str>,
    tpl_name: &str,
    tpl_bound: Option<&PhpType>,
    param_hint: Option<&PhpType>,
    subs: &HashMap<String, PhpType>,
) -> Option<&'d str> {
    let default = default_value?;
    if subs.contains_key(tpl_name) {
        return None;
    }
    // A template bounded by a type operator resolves against the one
    // literal it binds to, and an omitted argument has such a literal
    // whenever the parameter declares a scalar default — known at the
    // declaration site exactly as an explicit argument is known at the
    // call site.
    if type_operator_bound_literal(tpl_bound, default).is_some() {
        return Some(default);
    }
    match classify_template_binding(tpl_name, param_hint) {
        TemplateBindingMode::ClassStringInner => Some(default),
        // A `null` default binds `T` only when nothing else in the hint
        // takes the null: for `@param T|null $t = null` it is the `null`
        // alternative's, and `T` is left unbound.
        TemplateBindingMode::Direct => {
            let hint_takes_null = param_hint.is_some_and(|h| {
                matches!(h.kind(), TypeKind::Nullable(_))
                    || h.union_members().iter().any(|m| m.is_null())
            });
            ((default == "null" && !hint_takes_null) || default.ends_with("::class"))
                .then_some(default)
        }
        _ => None,
    }
}

/// Apply one binding mode for `tpl_name`, recording whatever it resolves
/// into `subs`.
///
/// Returns `false` without touching `subs` when the mode cannot bind the
/// argument it was given, which is what lets a union `@param` try its
/// alternatives in turn (see [`candidate_binding_modes`]).
#[allow(clippy::too_many_arguments)]
fn apply_binding_mode(
    subs: &mut HashMap<String, PhpType>,
    binding_mode: &TemplateBindingMode,
    tpl_name: &str,
    arg_text: &str,
    param_hint: Option<&PhpType>,
    bindings: &[(Atom, Atom)],
    walker_types: Option<ArgWalkerTypes<'_>>,
    ctx: &ResolutionCtx<'_>,
) -> bool {
    // Only consulted where the text-driven resolver came back empty, so a
    // caller that reached this through the AST pays for the walk exactly
    // on the arguments that would otherwise bind nothing.
    let from_walker = || walker_types.and_then(|lookup| lookup(arg_text));
    let bound = match *binding_mode {
        TemplateBindingMode::Direct => {
            bind_direct(tpl_name, arg_text, param_hint, ctx, from_walker)
        }
        TemplateBindingMode::GenericWrapper(ref wrapper_name, tpl_position) => {
            bind_generic_wrapper(
                tpl_name,
                wrapper_name,
                tpl_position,
                arg_text,
                param_hint,
                ctx,
                from_walker,
            )
        }
        TemplateBindingMode::CallableReturnType => {
            bind_callable_return_template(arg_text, param_hint, tpl_name, subs, bindings, ctx)
        }
        // `@param callable(...): array<TKey, TValue> $cb` (`mapWithKeys()`,
        // `mapToGroups()`) — bind from the key (0) or value (1) of the
        // callback's array-shaped return, not the whole return type. A bare
        // `: array` annotation carries no key/value information, but the
        // inferred return is narrowed by the body's own array (e.g.
        // `fn ($o): array => ['x' => $o]`).
        TemplateBindingMode::CallableReturnArrayPosition(position) => {
            Backend::infer_closure_return_type(arg_text, ctx)
                .and_then(|ret_type| extract_array_position(&ret_type, position))
        }
        // `@param Closure(T): void $cb` — the closure's parameter type
        // annotation at the given position, unless another argument already
        // bound the template.
        TemplateBindingMode::CallableParamType(position) => {
            if subs.contains_key(tpl_name) {
                None
            } else {
                bind_callable_param_template(arg_text, position, ctx)
            }
        }
        TemplateBindingMode::ArrayElement => bind_array_element(arg_text, ctx, from_walker),
        TemplateBindingMode::ClassStringInner => class_string_inner_binding(arg_text, ctx),
        TemplateBindingMode::ClassStringGeneric(ref wrapper_name, tpl_position) => {
            class_string_generic_binding(arg_text, wrapper_name, tpl_position, ctx)
        }
    };
    let Some(bound) = bound else {
        return false;
    };
    insert_or_union(subs, tpl_name.to_string(), bound);
    true
}

/// `@param T $bar`, or a hint that buries `T` deeper than the classifier
/// models (`array<string, array<string, T>>`).
fn bind_direct(
    tpl_name: &str,
    arg_text: &str,
    param_hint: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
    from_walker: impl FnOnce() -> Option<PhpType>,
) -> Option<PhpType> {
    let bare_template =
        param_hint.is_some_and(|h| matches!(h.kind(), TypeKind::Named(n) if &**n == tpl_name));
    if bare_template && let Some(literal) = literal_arg_type(arg_text.trim()) {
        return Some(literal);
    }
    let resolved_type = Backend::resolve_arg_text_to_type(arg_text, ctx).or_else(from_walker)?;

    // `resolve_arg_text_to_type` collapses any `[...]` literal to the bare
    // `array` keyword, which loses the argument's own keys. When the
    // template binds directly (e.g. `@template T of array<array-key,
    // mixed>` with `@param T $items`), that erased shape is the only
    // source of type information — there is no wrapping hint to unify
    // against — so build the literal's real key/value shape here instead,
    // letting `key-of<T>`/`value-of<T>` on the bound template project the
    // caller's actual keys.
    let literal_shape = resolved_type
        .is_bare_array()
        .then(|| array_literal_shape_type(arg_text, ctx))
        .flatten();

    // Binding the whole argument through a hint that is not just the
    // template name would re-wrap it, so unify the two shapes there.
    let unified = param_hint.filter(|_| !bare_template).and_then(|h| {
        // A union hint that offers an array-like alternative alongside the
        // bare template name (`iterable<array-key, T>|T`) still classifies
        // as `Direct`, because the bare alternative matches any argument.
        // An array *literal* argument resolves to a bare `array` with no
        // element type though, so unifying against it falls through to
        // `mixed` — unwrap the literal's elements the way `GenericWrapper`
        // binding does and retry before that fallback.  The elements are
        // widened the way a scalar argument through such a hint is.
        if resolved_type.is_bare_array()
            && let Some(elem) = array_literal_element_type(arg_text, ctx)
            && let Some(unified) = unify_template(
                h,
                &PhpType::array_of(elem.widen_scalar_literals()),
                tpl_name,
            )
        {
            return Some(unified);
        }
        unify_template(h, &resolved_type, tpl_name)
    });
    Some(unified.or(literal_shape).unwrap_or(resolved_type))
}

/// `@param T[] $items` or `@param array<T> $items`: `T` is one element.
fn bind_array_element(
    arg_text: &str,
    ctx: &ResolutionCtx<'_>,
    from_walker: impl FnOnce() -> Option<PhpType>,
) -> Option<PhpType> {
    if arg_text.starts_with('[') && arg_text.ends_with(']') {
        return array_literal_element_type(arg_text, ctx);
    }
    // The call-expression fallback covers arguments whose declared return
    // type is an array (`getConfigs()` returning `array<string, Config>`)
    // — those carry no class info, so the general resolver yields nothing.
    let resolved_type = Backend::resolve_arg_text_to_type(arg_text, ctx)
        .or_else(|| resolve_arg_call_raw_type(arg_text, ctx))
        .or_else(from_walker)?;
    array_element_binding(resolved_type)
}

/// `@param Wrapper<…, T, …> $w`: `T` is the argument's generic argument
/// at `tpl_position`, however the argument comes to be a `Wrapper`.
///
/// When nothing reads a `Wrapper` argument off the argument, `T` is left
/// unbound rather than bound to the whole argument, which would put the
/// container where its element belongs.
fn bind_generic_wrapper(
    tpl_name: &str,
    wrapper_name: &str,
    tpl_position: usize,
    arg_text: &str,
    param_hint: Option<&PhpType>,
    ctx: &ResolutionCtx<'_>,
    from_walker: impl FnOnce() -> Option<PhpType>,
) -> Option<PhpType> {
    // When the argument is a closure and the param hint union contains a
    // Callable variant (e.g. `iterable<T>|(Closure(): Generator<T>)`), try
    // yield inference first — before array-like or hierarchy extraction,
    // which would incorrectly bind `Closure`.
    if let Some(concrete) = Backend::try_closure_return_type_for_template(
        arg_text,
        tpl_name,
        tpl_position,
        param_hint,
        ctx,
    ) {
        return Some(concrete);
    }

    // `classify_template_binding` assigns positions by index in the hint's
    // generic args: `array<TKey, TValue>` → positions 0 and 1.  A wrapper
    // written with a single argument (`iterable<T>`) names only the value,
    // even though it sits at index 0.
    let arity = wrapper_arity(param_hint, wrapper_name);

    if is_array_like_wrapper(wrapper_name) {
        if arg_text.starts_with('[') && arg_text.ends_with(']') {
            // `resolve_arg_text_to_type("[1, 2, 3]")` returns a bare `array`,
            // so the literal is unwrapped and its entries resolved directly.
            return if arity >= 2 && tpl_position == 0 {
                resolve_array_literal_generic(0, arg_text, ctx)
            } else {
                array_literal_element_type(arg_text, ctx)
            };
        }

        // Resolve the argument's raw iterable type — from a variable's
        // annotations/assignments (`$users` as `array<int, User>`) or from
        // a call expression's declared return type — and extract the
        // positional generic argument.
        let resolved = resolve_arg_iterable_raw_type(arg_text, ctx).or_else(from_walker)?;
        // Walk the parameter hint and the argument type together first.
        // Positional extraction only unwraps one level, so it binds the
        // whole inner array for a hint like `array<string, array<string, T>>`.
        if let Some(unified) = param_hint
            .filter(|h| !names_template_directly(h, tpl_name))
            .and_then(|h| unify_template(h, &resolved, tpl_name))
        {
            return Some(unified);
        }
        // `array{}` has no entries to read a key or value off, which is
        // what `never` says.
        if resolved.is_empty_array_shape() {
            return Some(PhpType::never());
        }
        return match (arity, tpl_position) {
            (0 | 1, _) | (_, 1) => resolved.extract_value_type(false).cloned(),
            // `array<V>`, `V[]` and a bare `array` name no key type, so
            // their keys are any PHP allows rather than the `int` iteration
            // assumes. That is still a binding: left unbound, the template
            // would be taken from a callback's parameter annotation instead.
            (_, 0) if resolved.has_open_key_domain() => Some(PhpType::named(atom("array-key"))),
            (_, 0) => crate::type_engine::variable::array_func_rules::array_key_domain(&resolved),
            _ => None,
        };
    }

    let resolved = Backend::resolve_arg_text_to_type(arg_text, ctx).or_else(from_walker);

    // `class-string<T>` handed a `class-string<Foo>` binds `Foo`, not a
    // doubly wrapped `class-string<class-string<Foo>>`.
    if wrapper_name == "class-string" && tpl_position == 0 {
        let resolved = resolved?;
        return Some(match resolved.unwrap_class_string_inner() {
            Some(inner) => inner.clone(),
            None => resolved,
        });
    }

    // For a class wrapper the argument may be the wrapper itself
    // (`Container<Foo>`) or a class whose ancestry reaches it with concrete
    // arguments (`FooContainer extends Container<Foo>`).  A hint with a
    // single argument where the ancestor takes several (`Iterator<T>` for an
    // `Iterator<int, Foo>`) names the value, the last one.
    if let Some(args) = resolved
        .as_ref()
        .and_then(|ty| extract_generic_args_from_ancestor(ty, wrapper_name, ctx))
    {
        let arg = if arity == 1 && args.len() > 1 {
            args.last()
        } else {
            args.get(tpl_position)
        };
        return arg.cloned();
    }

    // `new Wrapper(new X())` binds the wrapper's own templates from its
    // constructor arguments.
    wrapper_constructor_template(wrapper_name, tpl_position, arg_text, ctx)
}

/// What `new Wrapper(…)` binds the wrapper's template at `tpl_position`
/// to, from the wrapper's own constructor arguments.
fn wrapper_constructor_template(
    wrapper_name: &str,
    tpl_position: usize,
    arg_text: &str,
    ctx: &ResolutionCtx<'_>,
) -> Option<PhpType> {
    let rest = arg_text.trim().strip_prefix("new")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let wrapper_cls = (ctx.class_loader)(wrapper_name).or_else(|| {
        ctx.all_classes
            .iter()
            .find(|c| crate::util::short_name(&c.name) == crate::util::short_name(wrapper_name))
            .cloned()
    })?;
    let wrapper_ctor = wrapper_cls.get_method("__construct")?;
    if wrapper_ctor.template_bindings.is_empty() {
        return None;
    }
    let paren_start = rest.find('(')?;
    let paren_end = rest.rfind(')')?;
    let inner_args = rest.get(paren_start + 1..paren_end)?.trim();
    let wrapper_arg_texts = crate::type_engine::conditional_resolution::split_text_args(inner_args);
    let wrapper_subs =
        bind_constructor_template_args(&wrapper_cls, wrapper_ctor, &wrapper_arg_texts, ctx);
    let wrapper_tpl = wrapper_cls.template_params.get(tpl_position)?;
    wrapper_subs.get(wrapper_tpl.as_str()).cloned()
}

/// The class `new` produces from `arg_texts`: its type, with each template
/// bound from the constructor arguments, and the class resolved against
/// those arguments.
///
/// A template no argument binds takes its declared default, else its bound,
/// else `mixed`, so a raw template name never reaches the members.
pub(crate) fn instantiate_class(
    cls: &ClassInfo,
    arg_texts: &[&str],
    ctx: &ResolutionCtx<'_>,
) -> (PhpType, Arc<ClassInfo>) {
    let subs = constructor_class_subs(cls, arg_texts, ctx);
    let mut type_args = crate::inheritance::default_type_args(cls);
    for (param, arg) in cls.template_params.iter().zip(type_args.iter_mut()) {
        if let Some(bound) = subs.get(param.as_str()) {
            *arg = bound.clone();
        }
    }
    let mut substituted = crate::virtual_members::resolve_class_fully_with_type_args(
        cls,
        ctx.class_loader,
        ctx.resolved_class_cache,
        &type_args,
    );

    // A `@mixin TParam` naming a template cannot be resolved while the class
    // is, because the mixin is not known until the template is bound.
    if !subs.is_empty()
        && cls
            .mixins
            .iter()
            .any(|m| cls.template_params.iter().any(|t| t == m.as_str()))
    {
        let generic_subs = crate::inheritance::build_generic_subs(cls, &type_args);
        let mixin_members = crate::virtual_members::phpdoc::resolve_template_param_mixins(
            cls,
            &generic_subs,
            ctx.class_loader,
        );
        if !mixin_members.is_empty() {
            crate::virtual_members::merge_virtual_members(
                Arc::make_mut(&mut substituted),
                mixin_members,
            );
        }
    }

    (PhpType::generic_atom(cls.fqn(), type_args), substituted)
}

/// `cls`'s templates as its constructor's arguments bind them, keyed by
/// `cls`'s own template names even when the constructor is inherited.
fn constructor_class_subs(
    cls: &ClassInfo,
    arg_texts: &[&str],
    ctx: &ResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    // The constructor is taken from the class that declares it, unsubstituted,
    // so its bindings still name that class's templates.
    let (ctor_owner, inherited) = if cls.get_method("__construct").is_some() {
        (None, false)
    } else {
        match crate::inheritance::ancestors(cls, ctx.class_loader)
            .find(|(_, parent)| parent.get_method("__construct").is_some())
        {
            Some((_, ancestor)) => (Some(ancestor), true),
            None => return HashMap::new(),
        }
    };
    let owner = ctor_owner.as_deref().unwrap_or(cls);
    let Some(ctor) = owner.get_method("__construct") else {
        return HashMap::new();
    };
    if ctor.template_bindings.is_empty() {
        return HashMap::new();
    }

    let subs = bind_constructor_template_args(owner, ctor, arg_texts, ctx);
    let mut subs = if inherited && !subs.is_empty() {
        crate::type_engine::variable::rhs_resolution::remap_inherited_ctor_subs(
            cls,
            &subs,
            ctx.class_loader,
        )
    } else {
        subs
    };
    if !subs.is_empty() {
        infer_templates_from_bound_args(cls, &mut subs);
    }
    subs
}

/// Bind the templates a bound's generic arguments name, from what the
/// bounded template itself was bound to.
///
/// `TIterator as Iterator<TKey, TValue>` bound to `Generator<int, string>`
/// makes `TKey` `int` and `TValue` `string`.
fn infer_templates_from_bound_args(cls: &ClassInfo, subs: &mut HashMap<String, PhpType>) {
    for (bound_param, bound_type) in cls.template_param_bounds.iter() {
        let TypeKind::Generic(bound) = bound_type.kind() else {
            continue;
        };
        let Some(concrete) = subs.get(bound_param.as_str()) else {
            continue;
        };
        let TypeKind::Generic(concrete) = concrete.kind() else {
            continue;
        };
        let inferred: Vec<(String, PhpType)> = bound
            .args
            .iter()
            .zip(concrete.args.iter())
            .filter_map(|(bound_arg, concrete_arg)| match bound_arg.kind() {
                TypeKind::Named(tpl_name)
                    if cls.template_params.contains(tpl_name)
                        && !subs.contains_key(tpl_name.as_str()) =>
                {
                    Some((tpl_name.to_string(), concrete_arg.clone()))
                }
                _ => None,
            })
            .collect();
        subs.extend(inferred);
    }
}
