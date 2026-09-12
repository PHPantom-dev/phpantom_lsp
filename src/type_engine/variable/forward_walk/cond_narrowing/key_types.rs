use super::*;

/// Resolve what a synthetic scope key promises, reading the scope but not
/// writing to it.
///
/// Dispatches on the key's *trailing* segment, because that is the access
/// that produces the key's type: `$a->items["0"]` is an array access whose
/// base is a property path, while `$a["0"]->items` is a property access
/// whose base is an array access.  Testing for `->` anywhere in the key
/// would route the former down the member path, which splits at the last
/// `->` and would look up a member literally named `items["0"]` — a name no
/// class declares, so a model with a magic `__get` answers it with `mixed`
/// and that bogus `mixed` becomes the authoritative type for the key.
pub(super) fn resolve_synthetic_key_type(
    key: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    if key.ends_with(']') {
        resolve_array_key_type(key, scope, ctx)
    } else if key.contains("->") {
        resolve_member_key_type(key, scope, ctx)
    } else if let Some((class_key, prop_name)) = split_static_property_key(key) {
        resolve_static_property_key_type(class_key, prop_name, ctx)
    } else {
        scope.get(key).to_vec()
    }
}

/// Split `self::$repo` into the class side (`self`) and the property name
/// (`repo`), or `None` when the key is not a static property path.
///
/// Only the trailing segment counts, so `self::$repo->name` (a property
/// read *through* a static property) is not one: it is a member path whose
/// base happens to be static, and `resolve_member_key_type` splits it.
fn split_static_property_key(key: &str) -> Option<(&str, &str)> {
    let pos = key.rfind("::$")?;
    let prop = &key[pos + 3..];
    if prop.is_empty() || prop.contains(|c: char| !(c.is_alphanumeric() || c == '_')) {
        return None;
    }
    Some((&key[..pos], prop))
}

/// Resolve what a static property's declaration promises.
///
/// `self`/`static` name the class the walk is inside; anything else is a
/// written class name resolved through the same path a type hint takes,
/// so an imported short name resolves like it does everywhere else.
fn resolve_static_property_key_type(
    class_key: &str,
    prop_name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    // `self`/`static` name the class the walk is already inside, so its
    // own `ClassInfo` is the owner. Looking the name up in the index
    // instead would miss a class the index does not carry — the document
    // being edited is exactly that case.
    if matches!(class_key, "self" | "static") {
        return static_property_hint_of(ctx.current_class, prop_name, ctx);
    }

    let class_name = match class_key {
        "parent" => match ctx.current_class.parent_class {
            Some(parent) => parent.to_string(),
            None => return Vec::new(),
        },
        other => other.to_string(),
    };
    let owners = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &PhpType::named(crate::atom::atom(&class_name)),
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    for owner in &owners {
        let resolved = static_property_hint_of(owner, prop_name, ctx);
        if !resolved.is_empty() {
            return resolved;
        }
    }
    Vec::new()
}

/// What `owner`'s declaration of `prop_name` promises.
fn static_property_hint_of(
    owner: &crate::types::ClassInfo,
    prop_name: &str,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let Some(hint) =
        crate::inheritance::resolve_property_type_hint(owner, prop_name, ctx.class_loader)
    else {
        return Vec::new();
    };
    let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &hint,
        &ctx.current_class.name,
        ctx.all_classes,
        ctx.class_loader,
    );
    match resolved.is_empty() {
        true => vec![ResolvedType::from_type_string(hint)],
        false => ResolvedType::from_classes_with_hint(resolved, hint),
    }
}

/// Resolve the element type an array-access key promises (`$a["k"]`,
/// `$a->items["0"]`, `$a["x"]["y"]`, `$a["x"][$i]`).
fn resolve_array_key_type(
    key: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    // Split off the *last* bracket segment so a nested access resolves its
    // base (`$a["x"]` of `$a["x"]["y"]`) through the same dispatcher.
    let Some((base_var, segment)) = narrowing::split_trailing_bracket(key) else {
        return Vec::new();
    };
    // A variable index names no shape entry, so only the container's
    // element type describes it.
    let key_name = narrowing::bracket_segment_literal(segment);

    // Only the leading variable of a path is ever assigned in the scope, so
    // a compound base has to be resolved the same way this key is.  Each
    // step drops one segment, so the recursion is bounded by the number of
    // segments in the key.
    // A static property is one of those bases: nothing ever assigns
    // `self::$cache` a scope entry, so without this the whole key comes
    // back unresolved and `isset(self::$cache[$k])` proves nothing about
    // the read it guards.
    let resolved_base;
    let base_types: &[ResolvedType] = match scope.get(base_var) {
        [] if base_var.contains("->")
            || base_var.ends_with(']')
            || split_static_property_key(base_var).is_some() =>
        {
            resolved_base = resolve_synthetic_key_type(base_var, scope, ctx);
            &resolved_base
        }
        from_scope => from_scope,
    };
    if base_types.is_empty() {
        return Vec::new();
    }
    // Look up the array key's type.  Prefer a precise shape entry
    // (`array{class: Foo}`); fall back to the generic element type
    // (`array<string, Foo>` → `Foo`); and finally to `mixed` for an
    // untyped array (plain `array`).  Seeding the untyped case is
    // what lets assertion / class-string narrowing apply to an
    // array-index subject whose element type is otherwise unknown
    // (e.g. `assertInstanceOf(X::class, $arr['k'])`).
    let mut key_results: Vec<ResolvedType> = Vec::new();
    for rt in base_types {
        let element_type = key_name
            .and_then(|name| rt.type_string.extract_shape_key_type(name))
            .or_else(|| rt.type_string.extract_value_type(false).cloned())
            // An empty shape has no entry any key could address, so the
            // read is a guaranteed miss and yields `null` — the same
            // answer the offset-read path gives.  Widening to `mixed`
            // here would leave an `isset($a['k'])` branch claiming the
            // key could be anything, because `mixed` survives the null
            // strip that the check's whole purpose is to license.
            .or_else(|| rt.type_string.is_empty_array_shape().then(PhpType::null))
            .or_else(|| rt.type_string.is_array_like().then(PhpType::mixed));
        let Some(element_type) = element_type else {
            continue;
        };
        let resolved_classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &element_type,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        if resolved_classes.is_empty() {
            ResolvedType::extend_unique(
                &mut key_results,
                vec![ResolvedType::from_type_string(element_type)],
            );
        } else {
            ResolvedType::extend_unique(
                &mut key_results,
                ResolvedType::from_classes_with_hint(resolved_classes, element_type),
            );
        }
    }
    key_results
}

/// Resolve what a member key's declaration promises, reading the scope
/// but not writing to it.
fn resolve_member_key_type(
    key: &str,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<ResolvedType> {
    let (head, is_call) = match key.strip_suffix("()") {
        Some(head) => (head, true),
        None => (key, false),
    };
    let Some(arrow_pos) = head.rfind("->") else {
        return Vec::new();
    };
    let obj_var = &head[..arrow_pos];
    let member_name = &head[arrow_pos + 2..];

    // Resolve the object part's type from scope.  Only the leading
    // variable of a path is ever assigned there, so a deeper path
    // (`$this->holder` in `$this->holder->service`, or `$rows["0"]` in
    // `$rows["0"]->name`) has to be resolved the same way this key is.
    // Each step drops one segment, so the recursion is bounded by the
    // number of segments in the key.
    let resolved_prefix;
    let obj_types: &[ResolvedType] = match scope.get(obj_var) {
        [] if narrowing::is_member_path_key(obj_var) => {
            resolved_prefix = resolve_synthetic_key_type(obj_var, scope, ctx);
            &resolved_prefix
        }
        from_scope => from_scope,
    };
    if obj_types.is_empty() {
        return Vec::new();
    }

    // Look up the member's type on the resolved class(es).
    let mut member_results: Vec<ResolvedType> = Vec::new();
    for rt in obj_types {
        let Some(ref cls) = rt.class_info else {
            continue;
        };
        let type_hint = if is_call {
            crate::inheritance::resolve_method_return_type(cls, member_name, ctx.class_loader)
        } else {
            crate::inheritance::resolve_property_type_hint(cls, member_name, ctx.class_loader)
        };
        let Some(hint) = type_hint else {
            continue;
        };
        // `self` / `static` / `$this` in the member's declared type name
        // the class the member was read off, not the class the reading
        // code happens to sit in.  `Scope::getParentScope(): ?self` seeded
        // against the enclosing class made a member lookup on the result
        // report a method missing from a class the code never mentions.
        let hint = if hint.contains_self_ref() {
            hint.resolve_self_refs(&cls.fqn(), cls.parent_class.as_ref().map(|p| p.as_str()))
        } else {
            hint
        };
        let resolved_classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
            &hint,
            &ctx.current_class.name,
            ctx.all_classes,
            ctx.class_loader,
        );
        if resolved_classes.is_empty() {
            if is_call && !hint_says_what_it_says(&hint, ctx.class_loader) {
                continue;
            }
            ResolvedType::extend_unique(
                &mut member_results,
                vec![ResolvedType::from_type_string(hint)],
            );
        } else {
            ResolvedType::extend_unique(
                &mut member_results,
                ResolvedType::from_classes_with_hint(resolved_classes, hint),
            );
        }
    }

    member_results
}

/// Whether a declared type that resolved to no class still means exactly
/// what it is written as, so seeding it as a call key's type is safe.
///
/// Two very different things resolve to no class.  One is a name standing
/// in for a type the declaration cannot resolve on its own — a `@template`
/// parameter, or an alias imported from elsewhere.  The call resolver at
/// the use site answers those properly by substituting from the receiver,
/// so seeding the unsubstituted name here would shadow a better answer
/// with a type no class stands behind.  The other is a type that is not a
/// name at all (`string|false`, `list<int>`, `array{a: int}`,
/// `int<0, max>`): nothing substitutes into it, so seeding it is what lets
/// a guard on a call key narrow the same way one on a property key
/// already does.
///
/// The test that separates them is whether every name the type mentions is
/// one we can account for: a keyword, or a class the loader knows.
/// `array<string, Foo>` passes, `T[]` does not.  An unevaluated type
/// operator (`key-of<X>`, `X[K]`) is excluded whatever it names, since its
/// meaning is still waiting on the operand.
fn hint_says_what_it_says(
    hint: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<crate::types::ClassInfo>>,
) -> bool {
    !hint.contains_unevaluated_operator() && every_name_is_known(hint, class_loader)
}

/// Whether every class-like name in `hint` is a keyword or loads.
fn every_name_is_known(
    hint: &PhpType,
    class_loader: &dyn Fn(&str) -> Option<std::sync::Arc<crate::types::ClassInfo>>,
) -> bool {
    let known = |name: &str| crate::php_type::is_keyword_type(name) || class_loader(name).is_some();
    match hint.kind() {
        TypeKind::Named(name) | TypeKind::StaticType(name) | TypeKind::ThisType(name) => {
            known(name)
        }
        TypeKind::Nullable(inner)
        | TypeKind::Array(inner)
        | TypeKind::ClassString(Some(inner))
        | TypeKind::InterfaceString(Some(inner)) => every_name_is_known(inner, class_loader),
        TypeKind::Union(members) | TypeKind::Intersection(members) => members
            .iter()
            .all(|member| every_name_is_known(member, class_loader)),
        TypeKind::Generic(generic) => {
            known(&generic.name)
                && generic
                    .args
                    .iter()
                    .all(|arg| every_name_is_known(arg, class_loader))
        }
        TypeKind::ArrayShape(entries) | TypeKind::ObjectShape(entries) => entries
            .iter()
            .all(|entry| every_name_is_known(&entry.value_type, class_loader)),
        TypeKind::Callable(callable) => {
            known(&callable.kind)
                && callable
                    .params
                    .iter()
                    .all(|param| every_name_is_known(&param.type_hint, class_loader))
                && callable
                    .return_type
                    .as_ref()
                    .is_none_or(|ret| every_name_is_known(ret, class_loader))
        }
        TypeKind::Literal(_)
        | TypeKind::IntRange(..)
        | TypeKind::ClassString(None)
        | TypeKind::InterfaceString(None) => true,
        // A conditional return type is decided by the arguments, and a
        // `Raw` node is text we could not parse; neither says anything a
        // scope key can hold.
        _ => false,
    }
}
