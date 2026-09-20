/// Instantiation (`new ClassName(…)`) resolution: the instantiated
/// class, constructor template-binding classification, and generic
/// substitution map construction for class-level `@template` parameters.
use std::collections::HashMap;
use std::sync::Arc;

use mago_syntax::cst::*;

use crate::Backend;
use crate::atom::{atom, bytes_to_str};
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

use super::array_access::{class_string_inner_binding, insert_or_union};
use super::calls::resolve_arg_call_raw_type;
use super::resolve_var_types;

/// Resolve `new ClassName(…)` to the instantiated class.
pub(super) fn resolve_rhs_instantiation(
    inst: &Instantiation<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let class_name = match inst.class {
        Expression::Self_(_) => Some("self".to_string()),
        Expression::Static(_) => Some("static".to_string()),
        Expression::Parent(_) => Some("parent".to_string()),
        Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
        _ => None,
    };
    if let Some(ref name) = class_name {
        let fqn = match name.as_str() {
            // The enclosing class's *fully qualified* name, not its short
            // one: a namespaced class whose short name collides with a
            // global class (`App\Error` vs the built-in `\Error`) would
            // otherwise have every `new self(…)` resolve to the global one.
            "self" | "static" => ctx.current_class.fqn().to_string(),
            // `parent` names the class the enclosing one extends, written
            // however the `extends` clause spelled it, so it resolves
            // through the same import table a written name does.
            "parent" => {
                let Some(parent) = ctx.current_class.parent_class else {
                    return vec![];
                };
                crate::util::resolve_source_class_name(
                    parent.as_str(),
                    ctx.current_class.file_namespace.as_deref(),
                    ctx.class_loader,
                )
            }
            other => crate::util::resolve_source_class_name(
                other,
                ctx.current_class.file_namespace.as_deref(),
                ctx.class_loader,
            ),
        };
        let parsed_name = if name == "static" {
            PhpType::static_type(atom(&fqn))
        } else {
            PhpType::named(atom(&fqn))
        };
        let classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &parsed_name,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );

        // ── Reflected property construction ─────────────────────
        // `new ReflectionProperty(C::class, 'name')` is the value
        // `ReflectionClass::getProperty('name')` builds, written the
        // other way, so it carries the same class and name.
        if classes.len() == 1
            && crate::type_engine::call_resolution::is_reflected_property_class(
                classes[0].fqn().as_str(),
            )
            && let Some(ref arg_list) = inst.argument_list
        {
            let arg_texts =
                crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                    arg_list,
                    ctx.content,
                );
            let arg_refs: Vec<&str> = arg_texts.iter().map(String::as_str).collect();
            let reflected = crate::type_engine::call_resolution::resolve_reflected_property_at_new(
                &classes[0],
                &arg_refs,
                &ctx.as_resolution_ctx(),
            );
            if let Some(ty) = reflected {
                return ResolvedType::from_classes_with_hint(classes, ty);
            }
        }

        // ── Constructor template inference ──────────────────────
        // When the class has `@template` params and the constructor
        // has `@param` bindings for them, infer concrete types from
        // the constructor arguments and apply the substitution to
        // the class so that methods returning `T` resolve correctly.
        //
        // The binding is matched against the declaration as written:
        // `type_hint_to_classes_typed` hands back the type the bare name
        // denotes, whose unsupplied parameters are already erased to
        // their bounds, leaving `@param array<TKey, TValue> $array` as
        // `array<mixed, mixed>` with nothing for the classifier to bind.
        let declaration = (classes.len() == 1 && !classes[0].template_params.is_empty())
            .then(|| {
                crate::type_engine::type_resolution::lookup_class_declaration(
                    &fqn,
                    &ctx.current_class.name,
                    ctx.all_classes,
                    ctx.class_loader,
                )
            })
            .flatten();
        if let Some(cls) = declaration.as_deref() {
            // Look for the constructor on the raw class first; if not
            // found (child class without its own constructor), walk up
            // the parent chain to find the original declaring class and
            // use its unsubstituted constructor.  This preserves the
            // original template param names in `template_bindings` so
            // that `classify_template_binding` can match them against
            // the parameter type hints (e.g. `array<T>` with binding
            // `("T", "$arr")`).
            let ancestor_cls_arc;
            let ctor_owner: &ClassInfo;
            let ctor_inherited;
            let ctor_ref = if let Some(c) = cls.get_method("__construct") {
                ctor_inherited = false;
                ctor_owner = cls;
                Some(c)
            } else {
                let mut found: Option<std::sync::Arc<ClassInfo>> = None;
                let mut cur = cls.parent_class.as_ref().map(|p| p.to_string());
                for _ in 0..15 {
                    let parent_name = match cur {
                        Some(ref n) => n.clone(),
                        None => break,
                    };
                    if let Some(parent) = (ctx.class_loader)(&parent_name) {
                        if parent.get_method("__construct").is_some() {
                            found = Some(parent);
                            break;
                        }
                        cur = parent.parent_class.as_ref().map(|p| p.to_string());
                    } else {
                        break;
                    }
                }
                match found {
                    Some(arc) => {
                        ancestor_cls_arc = arc;
                        ctor_inherited = true;
                        ctor_owner = &ancestor_cls_arc;
                        ancestor_cls_arc.get_method("__construct")
                    }
                    None => {
                        ctor_inherited = false;
                        ctor_owner = cls;
                        None
                    }
                }
            };
            if let Some(ctor) = ctor_ref
                && !ctor.template_bindings.is_empty()
                && let Some(ref arg_list) = inst.argument_list
            {
                let arg_texts =
                    crate::type_engine::variable::raw_type_inference::extract_arg_texts_from_ast(
                        arg_list,
                        ctx.content,
                    );
                if !arg_texts.is_empty() {
                    let rctx = ctx.as_resolution_ctx();
                    let raw_subs =
                        build_constructor_template_subs(ctor_owner, ctor, &arg_texts, &rctx, ctx);
                    // When the constructor is inherited, its template_bindings
                    // reference the ancestor's template param names.  Remap
                    // them to the child's template params via the @extends chain.
                    let subs = if ctor_inherited && !raw_subs.is_empty() {
                        remap_inherited_ctor_subs(cls, &raw_subs, ctx.class_loader)
                    } else {
                        raw_subs
                    };
                    if !subs.is_empty() {
                        // ── Infer unbound template params from bound constraints ──
                        // When a template param has a bound like
                        // `TIterator as Iterator<TKey, TValue>` and TIterator
                        // has been resolved to a concrete type (e.g.
                        // `Generator<int, string>`), match the concrete type's
                        // generic args against the bound's args to infer the
                        // nested template params (TKey=int, TValue=string).
                        let mut subs = subs;
                        for (bound_param, bound_type) in cls.template_param_bounds.iter() {
                            let bound_param_str: &str = bound_param.as_ref();
                            if let Some(concrete) = subs.get(bound_param_str).cloned()
                                && let TypeKind::Generic(bound) = bound_type.kind()
                            {
                                let concrete_args = match &concrete.kind() {
                                    TypeKind::Generic(g) => Some(g.args.as_slice()),
                                    _ => None,
                                };
                                if let Some(concrete_args) = concrete_args {
                                    for (i, bound_arg) in bound.args.iter().enumerate() {
                                        if let TypeKind::Named(tpl_name) = bound_arg.kind()
                                            && cls
                                                .template_params
                                                .iter()
                                                .any(|t| t.as_str() == tpl_name.as_str())
                                            && !subs.contains_key(tpl_name.as_str())
                                            && let Some(concrete_arg) = concrete_args.get(i)
                                        {
                                            subs.insert(tpl_name.to_string(), concrete_arg.clone());
                                        }
                                    }
                                }
                            }
                        }
                        let type_args: Vec<PhpType> = cls
                            .template_params
                            .iter()
                            .map(|p| {
                                let p_str: &str = p.as_ref();
                                subs.get(p_str).cloned().unwrap_or_else(|| {
                                    // Use the declared upper bound or `mixed`
                                    // instead of the raw template name so that
                                    // downstream consumers never see
                                    // `PhpType::named("TValue")`.
                                    cls.template_param_bounds
                                        .get(p)
                                        .cloned()
                                        .unwrap_or_else(PhpType::mixed)
                                })
                            })
                            .collect();
                        let substituted_arc =
                            crate::virtual_members::resolve_class_fully_with_type_args(
                                cls,
                                ctx.class_loader,
                                ctx.resolved_class_cache,
                                &type_args,
                            );
                        let mut substituted = Arc::unwrap_or_clone(substituted_arc);

                        // ── Template-param mixin resolution ────────────────
                        // When a class declares `@mixin TParam` where `TParam`
                        // is a template parameter, the mixin cannot be resolved
                        // during `resolve_class_fully` because the concrete type
                        // is not yet known.  Now that generic args are concrete,
                        // resolve those mixins and merge their members.
                        if cls
                            .mixins
                            .iter()
                            .any(|m| cls.template_params.iter().any(|t| t == m.as_str()))
                        {
                            let generic_subs =
                                crate::inheritance::build_generic_subs(cls, &type_args);
                            if !generic_subs.is_empty() {
                                let mixin_members =
                                    crate::virtual_members::phpdoc::resolve_template_param_mixins(
                                        cls,
                                        &generic_subs,
                                        ctx.class_loader,
                                    );
                                if !mixin_members.is_empty() {
                                    crate::virtual_members::merge_virtual_members(
                                        &mut substituted,
                                        mixin_members,
                                    );
                                }
                            }
                        }

                        let generic_type =
                            PhpType::generic_atom(substituted.fqn(), type_args.clone());
                        return vec![ResolvedType::from_both(generic_type, substituted)];
                    }
                }
            }

            // ── Fallback: resolve omitted template params ─────────
            // When no constructor argument bound any template param
            // (e.g. `new Collection()` with no args, or the
            // constructor has no template bindings), substitute all
            // template params with their declared default, upper bound,
            // or `mixed`. This prevents raw template names from leaking
            // into method parameter/return types.
            let type_args = crate::inheritance::default_type_args(cls);
            let substituted = crate::virtual_members::resolve_class_fully_with_type_args(
                cls,
                ctx.class_loader,
                ctx.resolved_class_cache,
                &type_args,
            );
            let generic_type = PhpType::generic_atom(substituted.fqn(), type_args.clone());
            return vec![ResolvedType::from_both_arc(generic_type, substituted)];
        }

        return ResolvedType::from_classes_with_hint(classes, parsed_name);
    }

    // ── `new $var` where `$var` holds a class-string ────────────
    // When the class expression is a variable, resolve it to check
    // if it holds a class-string value (e.g. `$f = Foo::class;
    // new $f`).  Extract the class name from the class-string and
    // use it to resolve the instantiated type.
    if let Expression::Variable(Variable::Direct(dv)) = inst.class {
        let var_name = bytes_to_str(dv.name).to_string();
        let resolved =
            crate::type_engine::variable::class_string_resolution::resolve_class_string_targets(
                &var_name,
                ctx.current_class,
                ctx.all_classes,
                ctx.content,
                ctx.cursor_offset,
                ctx.class_loader,
                ctx.backend,
            );
        if !resolved.is_empty() {
            return ResolvedType::from_classes(resolved.into_iter().map(Arc::new).collect());
        }

        // Fallback: resolve the variable's type and extract the inner
        // type from `class-string<T>`.  This handles parameters typed
        // as `@param class-string<Foo> $var` where there is no
        // `$var = Foo::class` assignment.
        let var_types = resolve_var_types(&var_name, ctx, ctx.cursor_offset);
        let class_name = extract_class_string_inner(&var_types);
        if let Some(name) = class_name
            && let Some(cls) = (ctx.class_loader)(&name)
        {
            return ResolvedType::from_classes(vec![cls]);
        }
    }

    vec![]
}

/// Extract the inner class name from a `class-string<T>` type in a list
/// of resolved types.  Handles `class-string<T>`, `?class-string<T>`,
/// and unions containing `class-string<T>`.
pub(super) fn extract_class_string_inner(resolved: &[ResolvedType]) -> Option<String> {
    resolved.iter().find_map(|rt| match &rt.type_string.kind() {
        TypeKind::ClassString(Some(inner)) => inner.base_name().map(|s| s.to_string()),
        TypeKind::Nullable(inner) => match inner.kind() {
            TypeKind::ClassString(Some(cs_inner)) => cs_inner.base_name().map(|s| s.to_string()),
            _ => None,
        },
        TypeKind::Union(members) => members.iter().find_map(|m| match m.kind() {
            TypeKind::ClassString(Some(inner)) => inner.base_name().map(|s| s.to_string()),
            TypeKind::Nullable(inner) => match inner.kind() {
                TypeKind::ClassString(Some(cs_inner)) => {
                    cs_inner.base_name().map(|s| s.to_string())
                }
                _ => None,
            },
            _ => None,
        }),
        _ => None,
    })
}

/// Extract a generic type argument from a class's ancestor chain.
///
/// Given an argument type (e.g. `FooContainer`) and a target wrapper class
/// (e.g. `Container`), walks the `@extends` chain to find where the argument
/// type (or one of its ancestors) extends the wrapper class, then extracts the
/// generic argument at `tpl_position`.
///
/// For example, if `FooContainer` has `@extends Container<Foo>`, calling
/// `extract_generic_arg_from_ancestor(FooContainer, "Container", 0, ...)` returns `Foo`.
pub(crate) fn extract_generic_arg_from_ancestor(
    arg_type: &PhpType,
    wrapper_name: &str,
    tpl_position: usize,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let class_name = match arg_type.kind() {
        TypeKind::Named(n) => n.as_str(),
        TypeKind::Generic(g) => g.name.as_str(),
        _ => return None,
    };

    // If the arg type itself is already generic with the wrapper name,
    // extract directly.  E.g. argument type is `Container<Foo>`.
    if let TypeKind::Generic(g) = arg_type.kind() {
        let n_short = crate::util::short_name(&g.name);
        let wrapper_short = crate::util::short_name(wrapper_name);
        if n_short.eq_ignore_ascii_case(wrapper_short) {
            return g.args.get(tpl_position).cloned();
        }
    }

    let class_loader = rctx.class_loader;
    let cls = class_loader(class_name)?;

    let wrapper_short = crate::util::short_name(wrapper_name);
    let mut visited = Vec::new();
    ancestor_generic_arg(
        &cls,
        wrapper_short,
        tpl_position,
        &HashMap::new(),
        &mut visited,
        class_loader,
    )
}

/// Maximum ancestry depth walked while looking for an ancestor's generic
/// argument.  A backstop against an `extends`/`implements` cycle the
/// loader hands back; the `visited` set is what actually bounds the work.
const MAX_ANCESTOR_GENERIC_DEPTH: usize = 15;

/// The type argument `ancestor_short` receives at `position`, as seen from
/// `cls`.
///
/// Walks the parent chain **and** the interface list, threading each
/// level's `@extends`/`@implements` arguments into the next, so a class
/// that reaches the ancestor only through an intermediate generic
/// interface still reports a concrete argument.
/// `X implements CollectorWithPaths<never, array{…}>` together with
/// `CollectorWithPaths extends Collector<TNodeType, TValue>` is what says
/// `Collector`'s value argument is that `array{…}`.
fn ancestor_generic_arg(
    cls: &ClassInfo,
    ancestor_short: &str,
    position: usize,
    subs: &HashMap<String, PhpType>,
    visited: &mut Vec<crate::atom::Atom>,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> Option<PhpType> {
    if visited.len() > MAX_ANCESTOR_GENERIC_DEPTH {
        return None;
    }
    let fqn = cls.fqn();
    if visited.contains(&fqn) {
        return None;
    }
    visited.push(fqn);

    if let Some(arg) = find_extends_generic_arg(cls, ancestor_short, position) {
        return Some(if subs.is_empty() {
            arg
        } else {
            arg.substitute(subs)
        });
    }

    for ancestor_name in cls.parent_class.iter().chain(cls.interfaces.iter()) {
        let Some(ancestor) = class_loader(ancestor_name) else {
            continue;
        };
        let next_subs = crate::inheritance::build_substitution_map(
            &crate::inheritance::ClassRef::Borrowed(cls),
            &ancestor,
            subs,
        );
        if let Some(arg) = ancestor_generic_arg(
            &ancestor,
            ancestor_short,
            position,
            &next_subs,
            visited,
            class_loader,
        ) {
            return Some(arg);
        }
    }

    None
}

/// Find a generic arg at `position` from a class's `@extends` generics
/// matching a target short name.
pub(super) fn find_extends_generic_arg(
    cls: &ClassInfo,
    target_short: &str,
    position: usize,
) -> Option<PhpType> {
    for (name, args) in cls
        .extends_generics
        .iter()
        .chain(cls.implements_generics.iter())
    {
        if crate::util::short_name(name) == target_short {
            return args.get(position).cloned();
        }
    }
    None
}

/// Remap constructor template substitutions from ancestor param names to child
/// param names when a constructor is inherited.
///
/// When `CollectionChild<T, V>` extends `Collection<V>` and `Collection` has
/// `@template T` with constructor `@param array<T> $arr`, the inherited
/// constructor's `template_bindings` map `("T", "$arr")` where `T` is
/// `Collection`'s template param.  After inference, `raw_subs` contains
/// `{"T" => Dog}`.  We need to translate this to `{"V" => Dog}` because
/// `Collection.T` maps to `CollectionChild.V` via `@extends Collection<V>`.
pub(crate) fn remap_inherited_ctor_subs(
    child: &ClassInfo,
    raw_subs: &HashMap<String, PhpType>,
    class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<ClassInfo>>,
) -> HashMap<String, PhpType> {
    // Walk up the extends chain to find the class that originally declares
    // the constructor, building a cumulative mapping from ancestor template
    // params to child template params.
    //
    // Start with an identity map for the child's own template params.
    let mut ancestor_to_child: HashMap<String, PhpType> = child
        .template_params
        .iter()
        .map(|p| (p.to_string(), PhpType::named(atom(p.as_ref()))))
        .collect();

    // Track the current node's extends info as owned data so we don't
    // need a reference across loop iterations.
    let mut cur_parent_class = child.parent_class;
    let mut cur_extends_generics = child.extends_generics.clone();

    for _ in 0..15 {
        let parent_name = match cur_parent_class {
            Some(ref p) => *p,
            None => break,
        };
        let parent = match class_loader(&parent_name) {
            Some(p) => p,
            None => break,
        };

        // Find @extends generics for this parent (e.g. @extends Collection<V>).
        let parent_short = crate::util::short_name(&parent.name);
        if let Some((_, type_args)) = cur_extends_generics
            .iter()
            .find(|(name, _)| crate::util::short_name(name) == parent_short)
        {
            // Build a mapping: parent.template_params[i] → type_args[i],
            // then resolve type_args through ancestor_to_child to get
            // parent param → child param.
            let mut new_mapping = HashMap::new();
            for (i, parent_param) in parent.template_params.iter().enumerate() {
                if let Some(arg) = type_args.get(i) {
                    let resolved = arg.substitute(&ancestor_to_child);
                    new_mapping.insert(parent_param.to_string(), resolved);
                }
            }
            ancestor_to_child = new_mapping;
        } else {
            // No @extends generics — can't map further.
            break;
        }

        // If the parent has the constructor, we've found our ancestor.
        if parent.get_method("__construct").is_some() {
            break;
        }

        cur_parent_class = parent.parent_class;
        cur_extends_generics = parent.extends_generics.clone();
    }

    // Now remap: for each entry in raw_subs (keyed by ancestor param name),
    // find which child param it maps to via ancestor_to_child.
    let mut result = HashMap::new();
    for (ancestor_param, inferred_type) in raw_subs {
        if let Some(child_type) = ancestor_to_child.get(ancestor_param) {
            // child_type is typically PhpType::named("V") — extract the name.
            match child_type.kind() {
                TypeKind::Named(child_param) => {
                    result.insert(child_param.to_string(), inferred_type.clone());
                }
                _ => {
                    // Complex mapping (e.g. mapped to a concrete type, not a
                    // param name) — keep the original key as fallback.
                    result.insert(ancestor_param.to_string(), inferred_type.clone());
                }
            }
        } else {
            // No mapping found — keep the original key.
            result.insert(ancestor_param.to_string(), inferred_type.clone());
        }
    }
    result
}

/// Build a template substitution map from constructor arguments.
///
/// Uses the constructor's `template_bindings` (from `@param T $name`
/// annotations) to match template parameters to their concrete types
/// inferred from the call-site arguments.  Handles:
///   - Direct type: `@param T $bar` + `new Foo(new Baz())` → `T = Baz`
///   - Array type: `@param T[] $items` + `new Foo([new X()])` → `T = X`
///   - Generic wrapper: `@param Wrapper<T> $w` + `new Foo(new Wrapper(new X()))` → `T = X`
///     (by resolving the wrapper's constructor template params recursively)
pub(super) fn build_constructor_template_subs(
    _class: &ClassInfo,
    ctor: &crate::types::MethodInfo,
    arg_texts: &[String],
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> HashMap<String, PhpType> {
    let mut subs = HashMap::new();

    // Bind the raw source-order argument texts to parameters by PHP's rules
    // so a named argument (`id: Foo::class`) is routed to the parameter it
    // targets rather than its ordinal slot, and its `name:` prefix is
    // stripped off the value.
    let arg_refs: Vec<&str> = arg_texts.iter().map(|s| s.as_str()).collect();
    let bound = crate::call_args::bind_text_args_to_params(&ctor.parameters, &arg_refs);

    for (tpl_name, param_name) in &ctor.template_bindings {
        let param_idx = match ctor
            .parameters
            .iter()
            .position(|p| p.name == param_name.as_str())
        {
            Some(idx) => idx,
            None => continue,
        };

        let provided_arg = bound.get(param_idx).and_then(|o| o.as_deref());

        let param_hint = ctor
            .parameters
            .get(param_idx)
            .and_then(|p| p.type_hint.as_ref());
        let binding_mode = classify_template_binding(tpl_name, param_hint);

        // Fall back to the parameter's default value only for binding
        // modes where the default is meaningful.
        let default_value = ctor
            .parameters
            .get(param_idx)
            .and_then(|p| p.default_value.as_deref());
        let arg_text: &str = match provided_arg {
            Some(text) => text,
            None => match &binding_mode {
                TemplateBindingMode::ClassStringInner => match default_value {
                    Some(d) => d,
                    None => continue,
                },
                TemplateBindingMode::Direct => match default_value {
                    Some(d) if d.ends_with("::class") => d,
                    _ => continue,
                },
                _ => continue,
            },
        };

        match binding_mode {
            TemplateBindingMode::Direct => {
                if let Some(resolved_type) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
                    subs.insert(tpl_name.to_string(), resolved_type);
                }
            }
            TemplateBindingMode::CallableReturnType => {
                if let Some(bound) =
                    crate::type_engine::call_resolution::bind_callable_return_template(
                        arg_text, param_hint, tpl_name, rctx,
                    )
                {
                    subs.insert(tpl_name.to_string(), bound);
                }
            }
            TemplateBindingMode::CallableReturnArrayPosition(position) => {
                // `@param callable(...): array<TKey, TValue> $cb` — bind
                // from the key/value of the callback's array-shaped
                // return, not the whole return type.
                if let Some(extracted) = Backend::infer_closure_return_type(arg_text, rctx)
                    .and_then(|ret_type| extract_array_position(&ret_type, position))
                {
                    subs.insert(tpl_name.to_string(), extracted);
                }
            }
            TemplateBindingMode::CallableParamType(position) => {
                if let Some(param_type) =
                    crate::type_engine::call_resolution::bind_callable_param_template(
                        arg_text, position, rctx,
                    )
                {
                    subs.insert(tpl_name.to_string(), param_type);
                }
            }
            TemplateBindingMode::ArrayElement => {
                // `@param T[] $items` — resolve individual array elements.
                if arg_text.starts_with('[') && arg_text.ends_with(']') {
                    let inner = arg_text[1..arg_text.len() - 1].trim();
                    if inner.is_empty() {
                        // Empty array `[]` → element type is `never`
                        // (an empty collection has no elements).
                        subs.insert(tpl_name.to_string(), PhpType::never());
                    } else {
                        let first_elem =
                            crate::type_engine::conditional_resolution::split_text_args(inner);
                        if let Some(elem) = first_elem.first()
                            && let Some(resolved_type) =
                                Backend::resolve_arg_text_to_type(elem.trim(), rctx)
                        {
                            subs.insert(tpl_name.to_string(), resolved_type);
                        }
                    }
                } else if let Some(resolved_type) =
                    Backend::resolve_arg_text_to_type(arg_text, rctx)
                        .or_else(|| resolve_arg_call_raw_type(arg_text, rctx))
                {
                    // Extract the element type from array-like types
                    // so we bind T to the element, not the whole array.
                    // The call-expression fallback covers arguments whose
                    // declared return type is an array (`getConfigs()`
                    // returning `array<string, Config>`) — those carry no
                    // class info, so the general resolver yields nothing.
                    if let Some(elem_type) = array_element_binding(resolved_type) {
                        insert_or_union(&mut subs, tpl_name.to_string(), elem_type);
                    }
                }
            }
            TemplateBindingMode::ClassStringInner => {
                if let Some(binding) = class_string_inner_binding(arg_text, rctx) {
                    insert_or_union(&mut subs, tpl_name.to_string(), binding);
                }
            }
            TemplateBindingMode::GenericWrapper(wrapper_name, tpl_position) => {
                if let Some(concrete) = Backend::try_closure_return_type_for_template(
                    arg_text,
                    tpl_name,
                    tpl_position,
                    param_hint,
                    rctx,
                ) {
                    subs.insert(tpl_name.to_string(), concrete);
                    continue;
                }
                // `@param array<TKey, T> $items` with `[]` → `never`.
                // An empty array literal has no keys or values, so all
                // generic type args of array-like wrappers are `never`.
                let is_array_like = matches!(
                    wrapper_name.as_str(),
                    "array" | "list" | "non-empty-array" | "non-empty-list"
                );
                if is_array_like
                    && arg_text.starts_with('[')
                    && arg_text.ends_with(']')
                    && arg_text[1..arg_text.len() - 1].trim().is_empty()
                {
                    subs.insert(tpl_name.to_string(), PhpType::never());
                } else if let Some(concrete) = resolve_generic_wrapper_template(
                    &wrapper_name,
                    tpl_position,
                    arg_text,
                    rctx,
                    ctx,
                ) {
                    subs.insert(tpl_name.to_string(), concrete);
                }
            }
        }
    }

    subs
}

/// What an `@param T[] $items` template binds to for an argument that
/// resolved to `resolved_type`.
///
/// A container binds the element type iteration would yield, so `T` names
/// one element rather than the whole array. Anything that is not a
/// container is itself the element (`@param T[] $items` bound from a
/// spread-like single value).
///
/// Returns `None` for a container whose elements cannot be read — a bare
/// `array`, or a union whose array-like members are all opaque. Leaving
/// `T` unbound lets it fall back to its declared bound (or `mixed`),
/// which beats binding it to the container and typing every callback
/// parameter as the array it iterates.
pub(crate) fn array_element_binding(resolved_type: PhpType) -> Option<PhpType> {
    if resolved_type
        .union_members()
        .iter()
        .any(|member| member.is_array_like())
    {
        return resolved_type.iterable_element_type();
    }
    Some(resolved_type)
}

/// How a template parameter is referenced in a `@param` type annotation.
#[derive(Debug, PartialEq)]
pub(crate) enum TemplateBindingMode {
    /// `@param T $bar` — the whole type is the template param.
    Direct,
    /// `@param T[] $items` — the template param is the array element type.
    ArrayElement,
    /// `@param Wrapper<..., T, ...> $a` — the template param is a generic
    /// argument of the wrapper class at the given position.
    GenericWrapper(String, usize),
    /// `@param callable(...): T $cb` — the template param appears in the
    /// callable's return type.  The binding is resolved by extracting the
    /// return type annotation from the closure/arrow-function argument.
    CallableReturnType,
    /// `@param callable(...): array<TKey, TValue> $cb` — the template
    /// param appears at a specific position (0 = key, 1 = value) of the
    /// callable's array-shaped return type, e.g. `mapWithKeys()`'s
    /// `callable(TValue, TKey): array<TMapWithKeysKey, TMapWithKeysValue>`.
    /// The binding is resolved from that position of the callback's
    /// inferred return type, rather than the whole return type the way
    /// `CallableReturnType` would (which would bind two distinct template
    /// params to the same, wrong, whole-array value).
    CallableReturnArrayPosition(usize),
    /// `@param Closure(T): void $cb` — the template param appears in the
    /// callable's parameter list at the given position (0-based).  The
    /// binding is resolved by extracting the closure's parameter type
    /// annotation at that index from the argument text.
    CallableParamType(usize),
    /// `@param class-string<T> $class` — the template param appears inside
    /// `class-string<>`.  The binding is resolved by unwrapping the
    /// `class-string<>` layer from the resolved argument type.
    ClassStringInner,
}

/// Classify how a template parameter name appears in a `@param` type hint.
///
/// Handles union types like `Arrayable<TKey, TValue>|iterable<TKey, TValue>|null`
/// by recursively inspecting the [`PhpType`] structure.
pub(crate) fn classify_template_binding(
    tpl_name: &str,
    param_hint: Option<&PhpType>,
) -> TemplateBindingMode {
    let hint = match param_hint {
        Some(h) => h,
        None => return TemplateBindingMode::Direct,
    };

    classify_from_php_type(tpl_name, hint)
}

/// Every binding site a `@param` annotation offers for `tpl_name`, most
/// likely first.
///
/// [`classify_template_binding`] has to answer with a single mode, so for a
/// union it picks one alternative and the rest are lost. But each
/// alternative of
/// `Collection<TKey, TValue>|EloquentCollection<TKey, TValue>|array<TKey, TValue>`
/// is a binding site in its own right, and which one applies is decided by
/// the argument, not by the order they were written in. The caller tries
/// these in turn and keeps the first that resolves.
///
/// Alternatives that do not name `tpl_name` at all are left out: they would
/// classify as [`Direct`](TemplateBindingMode::Direct) by default and bind
/// the *whole* argument type, which is worse than not binding — an
/// `array<TKey, …>` handed a `list<string>` would report
/// `array<array<int, string>, …>`.
pub(crate) fn candidate_binding_modes(
    tpl_name: &str,
    param_hint: Option<&PhpType>,
) -> Vec<TemplateBindingMode> {
    let primary = classify_template_binding(tpl_name, param_hint);
    let Some(hint) = param_hint else {
        return vec![primary];
    };
    let TypeKind::Union(members) = hint.kind() else {
        return vec![primary];
    };

    let mut modes = vec![primary];
    for member in members {
        if member.is_null() || !mentions_template(tpl_name, member) {
            continue;
        }
        let mode = classify_from_php_type(tpl_name, member);
        if !modes.contains(&mode) {
            modes.push(mode);
        }
    }
    modes
}

/// Whether `ty` names `tpl_name` anywhere inside it.
fn mentions_template(tpl_name: &str, ty: &PhpType) -> bool {
    if ty.is_named(tpl_name) {
        return true;
    }
    match ty.kind() {
        TypeKind::Nullable(inner) | TypeKind::Array(inner) => mentions_template(tpl_name, inner),
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(|m| mentions_template(tpl_name, m))
        }
        TypeKind::Generic(g) => g.args.iter().any(|a| mentions_template(tpl_name, a)),
        TypeKind::Callable(c) => {
            c.return_type
                .as_ref()
                .is_some_and(|r| mentions_template(tpl_name, r))
                || c.params
                    .iter()
                    .any(|p| mentions_template(tpl_name, &p.type_hint))
        }
        _ => false,
    }
}

/// Recursively classify how a template parameter name appears in a parsed
/// [`PhpType`].
pub(super) fn classify_from_php_type(tpl_name: &str, ty: &PhpType) -> TemplateBindingMode {
    match ty.kind() {
        TypeKind::Nullable(inner) => classify_from_php_type(tpl_name, inner),
        TypeKind::Union(members) => {
            let mut fallback: Option<TemplateBindingMode> = None;
            let mut has_direct = false;
            let mut has_class_string_inner = false;
            for member in members {
                if member.is_null() {
                    continue;
                }
                if member.is_named(tpl_name) {
                    has_direct = true;
                    continue;
                }
                let result = classify_from_php_type(tpl_name, member);
                if matches!(result, TemplateBindingMode::ClassStringInner) {
                    has_class_string_inner = true;
                }
                if !matches!(result, TemplateBindingMode::Direct) && fallback.is_none() {
                    fallback = Some(result);
                }
            }
            // `class-string<T>|T` — the argument may be a class name or
            // an instance.  ClassStringInner binding handles both: it
            // unwraps `class-string<Foo>` to `Foo` and binds instance
            // types directly, whereas Direct would keep the class-string
            // wrapper on a `Foo::class` argument.
            if has_direct && has_class_string_inner {
                return TemplateBindingMode::ClassStringInner;
            }
            // If the template name appears directly as a union member,
            // prefer Direct.  Direct always works regardless of what
            // the argument is, while CallableReturnType only works when
            // the argument is a closure.  This handles the common
            // Laravel `(Closure($this): T)|T|null` pattern in `when()`.
            if has_direct {
                return TemplateBindingMode::Direct;
            }
            fallback.unwrap_or(TemplateBindingMode::Direct)
        }
        TypeKind::Array(inner) => {
            if inner.is_named(tpl_name) {
                return TemplateBindingMode::ArrayElement;
            }
            // `(class-string<T>|T)[]` — detect a class-string<T>
            // alternative in the element type the same way it is
            // detected when it appears unwrapped.
            if matches!(
                classify_from_php_type(tpl_name, inner),
                TemplateBindingMode::ClassStringInner
            ) {
                return TemplateBindingMode::ClassStringInner;
            }
            TemplateBindingMode::Direct
        }
        TypeKind::Named(n) if n == tpl_name => TemplateBindingMode::Direct,
        TypeKind::Generic(g) => {
            let (wrapper_name, args) = (&g.name, &g.args);
            // `array<T>` (single arg) should be treated as ArrayElement,
            // not GenericWrapper — "array" is not a real class that can
            // be resolved for constructor inference.  Multi-arg forms
            // like `array<TKey, TValue>` stay as GenericWrapper so that
            // function-level template inference can extract each arg
            // from a concrete generic type (e.g. `array<int, Foo>`).
            let is_array_like = matches!(
                wrapper_name.to_ascii_lowercase().as_str(),
                "array" | "list" | "non-empty-array" | "non-empty-list"
            );
            if is_array_like && args.len() == 1 {
                if args[0].is_named(tpl_name) {
                    return TemplateBindingMode::ArrayElement;
                }
                // `array<class-string<T>|T|...>` (the shape of variadic
                // parameter hints, e.g. Mockery's `mock(...$args)`) —
                // detect a class-string<T> alternative nested in the
                // element type the same way it is detected unwrapped,
                // so a `Foo::class` argument binds T to Foo rather
                // than to class-string<Foo>.
                if matches!(
                    classify_from_php_type(tpl_name, &args[0]),
                    TemplateBindingMode::ClassStringInner
                ) {
                    return TemplateBindingMode::ClassStringInner;
                }
            }
            for (i, arg) in args.iter().enumerate() {
                if arg.is_named(tpl_name) {
                    return TemplateBindingMode::GenericWrapper(wrapper_name.to_string(), i);
                }
            }
            TemplateBindingMode::Direct
        }
        TypeKind::Callable(c) => {
            if let Some(rt) = &c.return_type {
                if let Some(position) = array_key_value_position(rt, tpl_name) {
                    return TemplateBindingMode::CallableReturnArrayPosition(position);
                }
                if type_contains_name(rt, tpl_name) {
                    return TemplateBindingMode::CallableReturnType;
                }
            }
            for (i, p) in c.params.iter().enumerate() {
                if type_contains_name(&p.type_hint, tpl_name) {
                    return TemplateBindingMode::CallableParamType(i);
                }
            }
            TemplateBindingMode::Direct
        }
        TypeKind::ClassString(Some(inner)) | TypeKind::InterfaceString(Some(inner)) => {
            if inner.is_named(tpl_name) {
                return TemplateBindingMode::ClassStringInner;
            }
            TemplateBindingMode::Direct
        }
        _ => TemplateBindingMode::Direct,
    }
}

/// Extract the key (position 0) or value (position 1) type of an
/// array-like return type, for [`TemplateBindingMode::CallableReturnArrayPosition`].
///
/// Delegates to [`PhpType::iterable_key_type`] / [`PhpType::iterable_element_type`]
/// rather than [`PhpType::extract_key_type`] / [`PhpType::extract_value_type`]
/// so an array *literal*'s inferred shape (`array{x: Order}`, from a
/// closure body like `['x' => $o]`) is destructured the same as an
/// explicit `array<K, V>` annotation.
pub(crate) fn extract_array_position(ty: &PhpType, position: usize) -> Option<PhpType> {
    match position {
        0 => ty.iterable_key_type(),
        1 => ty.iterable_element_type(),
        _ => None,
    }
}

/// Whether `ty` is a two-argument `array<K, V>`/`non-empty-array<K, V>`
/// type with `tpl_name` at position 0 (key) or 1 (value).
///
/// Restricted to the exact two-argument form: `list<T>`/`T[]` have no key
/// slot of their own, so a template param there is a plain array element
/// (handled by the `ArrayElement`/`GenericWrapper` modes), not this
/// key/value destructuring.
fn array_key_value_position(ty: &PhpType, tpl_name: &str) -> Option<usize> {
    let TypeKind::Generic(g) = ty.kind() else {
        return None;
    };
    let is_array_like = matches!(
        g.name.to_ascii_lowercase().as_str(),
        "array" | "non-empty-array"
    );
    if !is_array_like || g.args.len() != 2 {
        return None;
    }
    g.args.iter().position(|a| a.is_named(tpl_name))
}

/// Check whether a [`PhpType`] tree contains a [`TypeKind::Named`] with the
/// given name anywhere in its structure.
pub(crate) fn type_contains_name(ty: &PhpType, name: &str) -> bool {
    match ty.kind() {
        TypeKind::Named(n) => n == name,
        TypeKind::Nullable(inner) | TypeKind::Array(inner) => type_contains_name(inner, name),
        TypeKind::Union(members) | TypeKind::Intersection(members) => {
            members.iter().any(|m| type_contains_name(m, name))
        }
        TypeKind::Generic(g) => g.args.iter().any(|a| type_contains_name(a, name)),
        TypeKind::Callable(c) => {
            c.params
                .iter()
                .any(|p| type_contains_name(&p.type_hint, name))
                || c.return_type
                    .as_ref()
                    .is_some_and(|rt| type_contains_name(rt, name))
        }
        TypeKind::ClassString(Some(inner))
        | TypeKind::InterfaceString(Some(inner))
        | TypeKind::KeyOf(inner)
        | TypeKind::ValueOf(inner) => type_contains_name(inner, name),
        _ => false,
    }
}

/// Resolve a template param that appears inside a generic wrapper type.
///
/// For `@param Wrapper<T> $a` with argument `new Wrapper(new X())`,
/// recursively resolve the wrapper's constructor template params to
/// find the concrete type for the template param at `tpl_position`.
pub(super) fn resolve_generic_wrapper_template(
    wrapper_name: &str,
    tpl_position: usize,
    arg_text: &str,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Option<PhpType> {
    // ── Built-in array-like types ───────────────────────────────
    // `array`, `list`, `non-empty-array`, `non-empty-list` are not
    // real classes — infer key/value types directly from the array
    // literal argument.
    if matches!(
        wrapper_name,
        "array" | "list" | "non-empty-array" | "non-empty-list"
    ) {
        if let Some(result) = resolve_array_literal_generic(tpl_position, arg_text, rctx) {
            return Some(result);
        }
        // If the argument is not a literal (e.g. a variable), resolve its
        // type and extract the generic arg at the given position.
        if let Some(resolved) = Backend::resolve_arg_text_to_type(arg_text, rctx) {
            return extract_generic_arg_at_position(&resolved, tpl_position);
        }
        return None;
    }

    let wrapper_cls = (ctx.class_loader)(wrapper_name)
        .map(Arc::unwrap_or_clone)
        .or_else(|| {
            ctx.all_classes
                .iter()
                .find(|c| crate::util::short_name(&c.name) == crate::util::short_name(wrapper_name))
                .map(|c| ClassInfo::clone(c))
        })?;

    let wrapper_ctor = wrapper_cls.get_method("__construct")?;
    if wrapper_ctor.template_bindings.is_empty() {
        return None;
    }

    // Extract the constructor arguments from the argument text.
    // e.g. from `new Foobar(new X())` extract `new X()`.
    let paren_start = arg_text.find('(')?;
    let paren_end = arg_text.rfind(')')?;
    let inner_args = arg_text[paren_start + 1..paren_end].trim();

    let wrapper_arg_texts = crate::type_engine::conditional_resolution::split_text_args(inner_args)
        .into_iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let wrapper_subs =
        build_constructor_template_subs(&wrapper_cls, wrapper_ctor, &wrapper_arg_texts, rctx, ctx);

    let wrapper_tpl = wrapper_cls.template_params.get(tpl_position)?;
    wrapper_subs.get(wrapper_tpl.as_str()).cloned()
}

/// Extract a generic type argument from an array literal.
///
/// For `@param array<TKey, TValue> $kv` with argument `["a" => 1]`:
/// - `tpl_position == 0` → key type (`string`)
/// - `tpl_position == 1` → value type (`int`)
///
/// For single-param wrappers like `list<T>`, position 0 is the element type.
pub(super) fn resolve_array_literal_generic(
    tpl_position: usize,
    arg_text: &str,
    rctx: &crate::type_engine::resolver::ResolutionCtx<'_>,
) -> Option<PhpType> {
    let trimmed = arg_text.trim();

    let inner = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        let s = trimmed.strip_prefix("array(")?;
        s.strip_suffix(')')?.trim()
    };

    if inner.is_empty() {
        return Some(PhpType::never());
    }

    let elements = crate::type_engine::conditional_resolution::split_text_args(inner);

    let first = elements.first()?.trim();
    let has_keys = first.contains("=>");

    if has_keys {
        // Collect key types (position 0) or value types (position 1)
        // from the first element (sufficient for inference).
        let arrow_pos = first.find("=>")?;
        match tpl_position {
            0 => {
                let key_text = first[..arrow_pos].trim();
                Backend::resolve_arg_text_to_type(key_text, rctx)
            }
            1 => {
                let val_text = first[arrow_pos + 2..].trim();
                Backend::resolve_arg_text_to_type(val_text, rctx)
            }
            _ => None,
        }
    } else {
        // No keys — this is a list-style array.
        // Position 0 in `array<T>` or `list<T>` is the element type.
        // Position 0 in `array<TKey, TValue>` would be `int` (implicit key).
        // Position 1 in `array<TKey, TValue>` is the element type.
        match tpl_position {
            0 => Some(PhpType::named(atom("int"))),
            1 => Backend::resolve_arg_text_to_type(first, rctx),
            _ => None,
        }
    }
}

/// Extract the generic type argument at a given position from a resolved type.
///
/// For `array<int, string>` with position 0 → `int`, position 1 → `string`.
/// For `list<User>` with position 0 → `User`.
/// Also handles `PhpType::array_of(inner)` as a single-arg generic.
pub(super) fn extract_generic_arg_at_position(ty: &PhpType, position: usize) -> Option<PhpType> {
    match ty.kind() {
        TypeKind::Generic(g) => {
            // `list<T>` has a single arg (the value type).  When the
            // binding expects position 1 (value position of `array<K, V>`),
            // map it to position 0 of the list.  Position 0 of a list
            // is implicitly `int` (sequential keys).
            let is_list_like = matches!(
                g.name.to_ascii_lowercase().as_str(),
                "list" | "non-empty-list"
            );
            if is_list_like && g.args.len() == 1 {
                return match position {
                    0 => Some(PhpType::int()),
                    1 => g.args.first().cloned(),
                    _ => None,
                };
            }
            g.args.get(position).cloned()
        }
        TypeKind::Array(inner) if position == 0 => Some(inner.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_direct_param() {
        let ty = PhpType::parse("T");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::Direct));
    }

    #[test]
    fn classify_array_element() {
        let ty = PhpType::parse("T[]");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::ArrayElement));
    }

    #[test]
    fn classify_generic_wrapper() {
        let ty = PhpType::parse("Collection<T>");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::GenericWrapper(_, 0)));
    }

    #[test]
    fn classify_callable_return_type() {
        let ty =
            PhpType::parse("callable(TReduceInitial|TReduceReturnType, TValue): TReduceReturnType");
        let mode = classify_template_binding("TReduceReturnType", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_closure_return_type() {
        let ty = PhpType::parse("Closure(int, string): T");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_callable_param_type() {
        // Template appears only in params, not in return type — should be CallableParamType.
        let ty = PhpType::parse("callable(T): void");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableParamType(0)));
    }

    #[test]
    fn classify_callable_param_type_second_position() {
        let ty = PhpType::parse("Closure(int, T): void");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableParamType(1)));
    }

    #[test]
    fn classify_callable_return_type_preferred_over_param() {
        // When T appears in both params and return type, return type wins.
        let ty = PhpType::parse("callable(T): T");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_nullable_union_callable() {
        // Template in callable return type within a union.
        let ty = PhpType::parse("callable(int): T|null");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::CallableReturnType));
    }

    #[test]
    fn classify_class_string_or_direct_union() {
        // `class-string<T>|T` — a class name or an instance may be
        // passed; ClassStringInner binding handles both.
        let ty = PhpType::parse("class-string<T>|T");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::ClassStringInner));
    }

    #[test]
    fn classify_class_string_union_nested_in_array_element() {
        // The variadic-parameter shape: `array<class-string<T>|T|array<T>>`.
        let ty = PhpType::parse("array<class-string<T>|T|array<T>>");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::ClassStringInner));
    }

    #[test]
    fn classify_closure_or_direct_union_stays_direct() {
        // The Laravel `when()` pattern must keep preferring Direct.
        let ty = PhpType::parse("(Closure(int): T)|T|null");
        let mode = classify_template_binding("T", Some(&ty));
        assert!(matches!(mode, TemplateBindingMode::Direct));
    }

    #[test]
    fn classify_none_hint() {
        let mode = classify_template_binding("T", None);
        assert!(matches!(mode, TemplateBindingMode::Direct));
    }

    #[test]
    fn type_contains_name_simple() {
        let ty = PhpType::named(atom("Foo"));
        assert!(type_contains_name(&ty, "Foo"));
        assert!(!type_contains_name(&ty, "Bar"));
    }

    #[test]
    fn type_contains_name_nested_callable() {
        let ty = PhpType::parse("callable(int): Decimal");
        assert!(type_contains_name(&ty, "Decimal"));
        assert!(type_contains_name(&ty, "int"));
        assert!(!type_contains_name(&ty, "string"));
    }

    #[test]
    fn type_contains_name_union() {
        let ty = PhpType::parse("Foo|Bar|null");
        assert!(type_contains_name(&ty, "Foo"));
        assert!(type_contains_name(&ty, "Bar"));
        assert!(type_contains_name(&ty, "null"));
        assert!(!type_contains_name(&ty, "Baz"));
    }
}
