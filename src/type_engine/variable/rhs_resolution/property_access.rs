//! Property and class-constant access (`$this->prop`, `$obj?->prop`,
//! `Foo::$prop`, `Foo::BAR`) resolution: resolves the member's type, plus the
//! `find_*_this_property_assignment*` scanners that narrow a property's
//! type from its last assignment in the enclosing method when the
//! assigned type is narrower than the declared one (or the property is
//! untyped).

use std::collections::HashSet;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::{Atom, atom, bytes_to_str};
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::parser::with_parsed_program;
use crate::php_type::{PhpType, TypeKind};
use crate::types::{ClassInfo, ResolvedType};

use crate::type_engine::resolver::VarResolutionCtx;

use super::nullsafe::{ReceiverNullability, apply_nullsafe_short_circuit};
use super::{resolve_rhs_expression, resolved_type_with_lookup};

/// Resolve property access: `$this->prop`, `$obj->prop`, `$obj?->prop`,
/// and the class-level reads `Foo::$prop` and `Foo::BAR`.
pub(super) fn resolve_rhs_property_access(
    access: &Access<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // A member named at runtime rather than spelled out — see
    // `runtime_named_member_type`.  `Cls::$prop` writes its property name
    // as a variable token, so only an indirect one (`Cls::${$name}`) is
    // actually dynamic.
    match access {
        Access::ClassConstant(cca) => match &cca.constant {
            ClassLikeConstantSelector::Identifier(ident) => {
                class_constant_access_type(cca, bytes_to_str(ident.value), ctx)
            }
            _ => super::runtime_named_member_type(),
        },
        Access::StaticProperty(spa) => match &spa.property {
            Variable::Direct(dv) => {
                let name = bytes_to_str(dv.name);
                static_property_type(spa, name.strip_prefix('$').unwrap_or(name), ctx)
            }
            _ => super::runtime_named_member_type(),
        },
        Access::Property(PropertyAccess {
            object, property, ..
        })
        | Access::NullSafeProperty(NullSafePropertyAccess {
            object, property, ..
        }) => match property {
            ClassLikeMemberSelector::Identifier(ident) => {
                let nullsafe = matches!(access, Access::NullSafeProperty(_));
                instance_property_type(object, bytes_to_str(ident.value), nullsafe, ctx)
            }
            _ => super::runtime_named_member_type(),
        },
    }
}

/// Resolve a property's type to `Vec<ResolvedType>`, preserving the
/// property's type hint string in each result.
///
/// When the property type is a scalar (e.g. `string`, `int`) and
/// `type_hint_to_classes_typed` returns no `ClassInfo`, a type-string-only
/// `ResolvedType` is produced so that the type information is not lost.
fn resolve_property_with_hint(
    prop_name: &str,
    owner: &ClassInfo,
    all_classes: &[Arc<ClassInfo>],
    class_loader: &ResolvedClassLoader<'_>,
) -> Vec<ResolvedType> {
    let type_hint = crate::inheritance::resolve_property_type_hint(owner, prop_name, class_loader);
    let resolved = crate::type_engine::type_resolution::resolve_property_types(
        prop_name,
        owner,
        all_classes,
        class_loader,
    );
    if resolved.is_empty() {
        // The property has a type hint but no matching class was
        // found (e.g. `list<Widget>`, `int`, `array{name: string}`).
        // Return a type-string-only entry so the type information
        // is not lost.
        return match type_hint {
            Some(hint) => {
                vec![resolved_type_with_lookup(hint, all_classes, class_loader)]
            }
            _ => vec![],
        };
    }
    match type_hint {
        Some(hint) => ResolvedType::from_classes_with_hint(resolved, hint),
        None => ResolvedType::from_classes(resolved),
    }
}

/// Class constant / enum case access: `Foo::BAR`.  Resolve the class and
/// check whether the constant is an enum case (→ type is the enum
/// itself) or a class constant (→ the type it holds).  `Foo::class` is
/// the class's name.
fn class_constant_access_type(
    cca: &ClassConstantAccess<'_>,
    const_name: &str,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class_fqn = ctx.current_class.fqn();
    let all_classes = ctx.file.all_classes;
    let class_loader = ctx.file.class_loader;
    let is_class_fetch = crate::parser::is_class_fetch(cca);

    let target_classes = match crate::class_lookup::class_expression_name(
        cca.class,
        ctx.current_class,
        ctx.file.all_classes,
        ctx.file.source_class_loader,
        ctx.file.source_imports,
    ) {
        Some(class_name) => {
            let resolved_name = class_name.strip_prefix('\\').unwrap_or(&class_name);
            let resolved_typed = PhpType::named(atom(resolved_name));
            let target_classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                &resolved_typed,
                &current_class_fqn,
                all_classes,
                class_loader,
            );

            // The magic `::class` constant yields the fully-qualified name
            // of the class as a `class-string<T>`. Resolving it to a plain
            // `string` would discard the class identity, so downstream
            // consumers (array element inference, `??` fallbacks, and
            // `class-string<object>` parameters) keep the concrete class.
            if is_class_fetch {
                // The identifier is spelled as the source writes it, which
                // for a name reached through a namespace import
                // (`Support\Pen` behind `use App\Support;`) is neither the
                // FQCN nor resolvable on its own once the class-string
                // leaves this file's context.  Prefer the resolved class.
                let name = match target_classes.first() {
                    Some(cls) => cls.fqn(),
                    None => atom(resolved_name),
                };
                // A written name, `self` and `parent` each name one class,
                // so the constant is exactly that class's name. `static`
                // is whichever subclass the call was made on, and a
                // trait's `self` whichever class uses the trait.
                let names_one_class = match cca.class {
                    Expression::Static(_) => false,
                    Expression::Self_(_) | Expression::Parent(_) => {
                        ctx.current_class.kind != crate::types::ClassLikeKind::Trait
                    }
                    _ => true,
                };
                let ty = if names_one_class {
                    PhpType::class_name_literal(name)
                } else {
                    PhpType::class_string(Some(PhpType::named(name)))
                };
                return vec![ResolvedType::from_type_string(ty)];
            }
            target_classes
        }
        // `$value::class` / `$value::CONST` (PHP 8.0+): the class is
        // whatever the expression holds.
        None => {
            let held = resolve_rhs_expression(cca.class, ctx);
            if is_class_fetch {
                return class_fetch_on_value(&held);
            }
            let mut classes: Vec<Arc<ClassInfo>> = Vec::new();
            for rt in &held {
                if let Some(cls) = &rt.class_info
                    && !classes.iter().any(|c| c.fqn() == cls.fqn())
                {
                    classes.push(Arc::clone(cls));
                }
            }
            if classes.is_empty() {
                classes = crate::type_engine::variable::class_string_resolution::classes_named_by(
                    &held, ctx,
                );
            }
            classes
        }
    };

    // Search local classes first.  If the constant is not found, resolve
    // via full inheritance merging so that constants from parent classes
    // are visible (e.g. `self::PARENT_CONST` in a subclass).
    let merged_classes: Vec<Arc<ClassInfo>>;
    let all_candidates: &[Arc<ClassInfo>] = if target_classes
        .iter()
        .any(|cls| cls.constants.iter().any(|c| c.name == const_name))
    {
        &target_classes
    } else {
        merged_classes = target_classes
            .iter()
            .map(|cls| {
                crate::virtual_members::resolve_class_fully_maybe_cached(
                    cls,
                    class_loader,
                    ctx.resolved_class_cache,
                )
            })
            .collect();
        &merged_classes
    };

    for cls in all_candidates {
        // Check if the constant is an enum case — the result type is the
        // enum class itself.
        if let Some(c) = cls.constants.iter().find(|c| c.name == const_name) {
            if c.is_enum_case {
                return ResolvedType::from_classes(target_classes);
            }
            if let Some(ts) = crate::type_engine::call_resolution::class_constant_type(
                cls,
                const_name,
                &ctx.as_resolution_ctx(),
            ) {
                let resolved = crate::type_engine::type_resolution::type_hint_to_classes_typed(
                    &ts,
                    &current_class_fqn,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return ResolvedType::from_classes_with_hint(resolved, ts);
                }
                return vec![ResolvedType::from_type_string(ts)];
            }
        }
    }
    vec![]
}

/// Static property access: `self::$prop`, `static::$prop`, `Foo::$prop`.
fn static_property_type(
    spa: &StaticPropertyAccess<'_>,
    prop_name: &str,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let Some(class_name) = crate::class_lookup::class_expression_name(
        spa.class,
        ctx.current_class,
        ctx.file.all_classes,
        ctx.file.source_class_loader,
        ctx.file.source_imports,
    ) else {
        return vec![];
    };
    let resolved_name = class_name.strip_prefix('\\').unwrap_or(&class_name);
    let resolved_typed = PhpType::named(atom(resolved_name));
    let target_classes = crate::type_engine::type_resolution::type_hint_to_classes_typed(
        &resolved_typed,
        &ctx.current_class.fqn(),
        ctx.file.all_classes,
        ctx.file.class_loader,
    );
    // `self::`, `static::`, and `parent::` all forward late static
    // binding, so a property typed `static` stays open; an explicit
    // `ClassName::` pins the class, collapsing it just like a fixed
    // method call (see `lsb_class_for_call`).
    let forwards_lsb = matches!(
        spa.class,
        Expression::Self_(_) | Expression::Static(_) | Expression::Parent(_)
    );
    for cls in &target_classes {
        let resolved =
            resolve_property_with_hint(prop_name, cls, ctx.file.all_classes, ctx.file.class_loader);
        if !resolved.is_empty() {
            return if forwards_lsb {
                resolved
            } else {
                resolved
                    .into_iter()
                    .map(|mut rt| {
                        rt.type_string = rt.type_string.replace_self_bound(&cls.fqn(), None);
                        rt
                    })
                    .collect()
            };
        }
    }
    vec![]
}

/// Instance property access, `$obj->prop` or (`nullsafe`) `$obj?->prop`:
/// what the read is known to hold here when it is narrower than the
/// declaration, else the declared type on each class the receiver can be.
fn instance_property_type(
    object: &Expression<'_>,
    prop_name: &str,
    nullsafe: bool,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    if let Some(narrowed) = narrowed_property_read(object, prop_name, ctx) {
        return narrowed;
    }
    let receiver = PropertyReceiver::of(object, prop_name, ctx);
    let mut resolved = receiver.declared_property_types(prop_name, ctx);
    apply_nullsafe_short_circuit(&mut resolved, receiver.nullability, nullsafe, object);
    resolved
}

/// What a read of `prop_name` on `object` is known to hold here, more
/// precisely than the property's declaration: the literal an enum case's
/// `->value` or `->name` is, or what `$this->prop` was last assigned in
/// the enclosing method.
fn narrowed_property_read(
    object: &Expression<'_>,
    prop_name: &str,
    ctx: &VarResolutionCtx<'_>,
) -> Option<Vec<ResolvedType>> {
    let class_loader = ctx.file.class_loader;

    // ── `Enum::CASE->value` / `Enum::CASE->name` ────────
    if let Expression::Access(Access::ClassConstant(cca)) = object
        && let ClassLikeConstantSelector::Identifier(case) = &cca.constant
        && let Some(enum_name) = crate::class_lookup::class_expression_name(
            cca.class,
            ctx.current_class,
            ctx.file.all_classes,
            ctx.file.source_class_loader,
            ctx.file.source_imports,
        )
        && let Some(enum_cls) = class_loader(Fqn::new(&enum_name))
        && let Some(literal) = crate::type_engine::call_resolution::enum_case_property_literal(
            &enum_cls,
            bytes_to_str(case.value),
            prop_name,
        )
    {
        return Some(vec![ResolvedType::from_type_string(literal)]);
    }

    // ── $this->prop assignment narrowing ────────────────
    // When the object is `$this`, check if there is an assignment to
    // `$this->propName` before the cursor in the current method.  If so,
    // use the assigned value's type — but ONLY when it is narrower (a
    // subtype of) the declared property type.  This handles patterns
    // like:
    //   $this->mock = $this->createMock(Foo::class);
    //   new Bar($this->mock); // mock is MockObject&Foo
    //
    // We reject widening assignments (e.g. narrowed type is `object` but
    // declared type is `Foo`) to avoid losing declared type information.
    //
    // A property that only exists through `__set` / `__get` is excluded:
    // the setter is free to transform, reroute, or drop the value, so the
    // assignment says nothing about what a later read returns.
    let Expression::Variable(Variable::Direct(dv)) = object else {
        return None;
    };
    if dv.name != b"$this"
        || crate::virtual_members::property_write_is_magic(
            ctx.current_class,
            prop_name,
            class_loader,
            ctx.resolved_class_cache,
        )
    {
        return None;
    }
    let narrowed = try_resolve_this_property_from_assignment(prop_name, ctx);
    if narrowed.is_empty() {
        return None;
    }
    // Look up the declared property type so we can verify the narrowed
    // type is actually narrower.  With no declared type (an untyped
    // property) the narrowed type is the best we have; one wider than
    // declared falls through to normal property type resolution.
    let declared_type =
        crate::class_lookup::find_class_by_fqn(ctx.file.all_classes, &ctx.current_class.fqn())
            .and_then(|cls| {
                crate::inheritance::resolve_property_type_hint(cls, prop_name, class_loader)
            });
    match declared_type {
        Some(declared)
            if !narrowed
                .iter()
                .all(|rt| fits_declared_type(&rt.type_string, &declared, class_loader)) =>
        {
            None
        }
        _ => Some(narrowed),
    }
}

/// Whether a type assigned to a property is a subtype of its declared
/// type.  Structural subtyping first, then the nominal class hierarchy.
fn fits_declared_type(
    assigned: &PhpType,
    declared: &PhpType,
    class_loader: &ResolvedClassLoader<'_>,
) -> bool {
    // Structural check covers scalars, unions, intersections, nullable,
    // generic, etc.
    if assigned.is_subtype_of(declared) {
        return true;
    }
    // Nominal check: if both are class-like, walk the class hierarchy.
    if let Some(assigned_base) = assigned.base_name()
        && let Some(cls) = class_loader(Fqn::new(assigned_base))
        && let Some(declared_base) = declared.base_name()
    {
        return crate::class_lookup::is_subtype_of(&cls, declared_base, class_loader);
    }
    // Intersection types: if any member satisfies the declared type, the
    // intersection does too.
    if let TypeKind::Intersection(members) = assigned.kind() {
        return members
            .iter()
            .any(|m| fits_declared_type(m, declared, class_loader));
    }
    false
}

/// The classes a property read's receiver can be, and what the read
/// needs to know about the receiver beyond them.
struct PropertyReceiver {
    owners: Vec<Arc<ClassInfo>>,
    /// Whether late static binding is still open on the receiver, as for a
    /// method call (see `lsb_class_for_call`): `$this`, or a receiver that
    /// is itself a bound `static(X)`, keeps a property typed `static`
    /// open; any other receiver fixes it to the class it names.
    lsb_open: bool,
    nullability: ReceiverNullability,
}

impl PropertyReceiver {
    /// Resolve the receiver `object` of a read of `prop_name`.
    fn of(object: &Expression<'_>, prop_name: &str, ctx: &VarResolutionCtx<'_>) -> Self {
        let mut receiver = Self {
            owners: Vec::new(),
            lsb_open: false,
            nullability: ReceiverNullability::Never,
        };
        receiver.owners = match object {
            Expression::Variable(Variable::Direct(dv)) if dv.name == b"$this" => {
                receiver.lsb_open = true;
                this_property_owners(prop_name, ctx)
            }
            Expression::Variable(Variable::Direct(dv)) => {
                let var = bytes_to_str(dv.name);
                // Check match-arm narrowing override first.
                if let Some(overridden) = ctx.match_arm_narrowing.get(var).cloned() {
                    receiver.classes_of(overridden)
                } else {
                    // When a scope_var_resolver is available (forward-walker
                    // RHS resolution), try it first so we read from the
                    // in-progress ScopeState instead of the diagnostic
                    // scope cache or backward scanner.
                    let from_scope = match ctx.scope_var_resolver {
                        Some(resolver) if var.starts_with('$') => resolver(var),
                        Some(resolver) => resolver(&format!("${var}")),
                        None => vec![],
                    };
                    let classes = receiver.classes_of(from_scope);
                    if !classes.is_empty() {
                        classes
                    } else {
                        receiver.classes_of(crate::type_engine::resolver::resolve_target_classes(
                            var,
                            crate::types::AccessKind::Arrow,
                            &ctx.as_resolution_ctx(),
                        ))
                    }
                }
            }
            // Handle non-variable object expressions like
            // `(new Canvas())->easel`, `getService()->prop`, or
            // `SomeClass::make()->prop` by recursively resolving the
            // expression type.
            _ => receiver.classes_of(resolve_rhs_expression(object, ctx)),
        };
        receiver
    }

    /// The classes `resolved` names, noting what it says about the
    /// receiver's nullability and late static binding.
    fn classes_of(&mut self, resolved: Vec<ResolvedType>) -> Vec<Arc<ClassInfo>> {
        if !resolved.is_empty() {
            self.nullability = ReceiverNullability::of(&resolved);
        }
        self.lsb_open |= resolved.iter().any(|rt| {
            matches!(
                rt.type_string.kind(),
                TypeKind::StaticType(_) | TypeKind::ThisType(_)
            )
        });
        ResolvedType::into_arced_classes(resolved)
    }

    /// The declared type of `prop_name` on each class the receiver can be,
    /// without duplicates.
    fn declared_property_types(
        &self,
        prop_name: &str,
        ctx: &VarResolutionCtx<'_>,
    ) -> Vec<ResolvedType> {
        let mut all_resolved: Vec<ResolvedType> = Vec::new();
        for owner in &self.owners {
            let resolved = resolve_property_with_hint(
                prop_name,
                owner,
                ctx.file.all_classes,
                ctx.file.class_loader,
            );
            for mut rt in resolved {
                // A synthetic owner (`__object_shape`) leaves `self` to the
                // caller's context, as `replace_self_in_property_type` does.
                if !self.lsb_open && !owner.name.starts_with("__") {
                    rt.type_string = rt.type_string.replace_self_bound(&owner.fqn(), None);
                }
                // The receiver's generic arguments may be what finally lets
                // an operator in the declaration be read (`Data[value-of<T>]`
                // with `T` bound to an enum case).
                if let Some(evaluated) =
                    crate::type_engine::call_resolution::evaluate_constant_operands(
                        &rt.type_string,
                        &ctx.as_resolution_ctx(),
                    )
                {
                    rt.type_string = evaluated;
                }
                if !all_resolved
                    .iter()
                    .any(|existing| existing.type_string == rt.type_string)
                {
                    all_resolved.push(rt);
                }
            }
        }
        all_resolved
    }
}

/// The classes whose `prop_name` a `$this->prop_name` read reaches: the
/// current class and, inside a trait, the classes its users are
/// guaranteed to be that declare it.
fn this_property_owners(prop_name: &str, ctx: &VarResolutionCtx<'_>) -> Vec<Arc<ClassInfo>> {
    let mut owners: Vec<Arc<ClassInfo>> =
        crate::class_lookup::find_class_by_fqn(ctx.file.all_classes, &ctx.current_class.fqn())
            .map(Arc::clone)
            .into_iter()
            .collect();
    // Inside a trait `$this` is also whatever the trait's users are
    // guaranteed to be, whose properties the trait's code reads like its
    // own.  A private one stays out of reach: the trait runs as part of a
    // class that only inherits it.  The walker seeds `$this` with those
    // bounds once per body, so read them from its scope rather than
    // searching the trait's users again for every property read.
    let bounds = if ctx.current_class.kind != crate::types::ClassLikeKind::Trait {
        Vec::new()
    } else if let Some(resolver) = ctx.scope_var_resolver {
        let trait_fqn = ctx.current_class.fqn();
        ResolvedType::into_arced_classes(resolver("$this"))
            .into_iter()
            .filter(|cls| cls.fqn() != trait_fqn)
            .collect()
    } else {
        crate::type_engine::trait_context::trait_this_bounds(
            ctx.current_class,
            ctx.file.all_classes,
            ctx.file.class_loader,
            ctx.file.backend,
        )
    };
    for bound in bounds {
        let merged = crate::virtual_members::resolve_class_fully_maybe_cached(
            &bound,
            ctx.file.class_loader,
            ctx.resolved_class_cache,
        );
        if merged
            .get_property(prop_name)
            .is_some_and(|p| p.visibility != crate::types::Visibility::Private)
        {
            owners.push(bound);
        }
    }
    owners
}

/// The type of `$value::class` given the types `$value` holds:
/// `class-string<T>` for each object type `T`, and a bare `class-string`
/// for `object` or an object shape, whose class is unknown.
///
/// A non-object member (`null`, a scalar) throws at runtime rather than
/// producing a class name, so it contributes nothing.
fn class_fetch_on_value(held: &[ResolvedType]) -> Vec<ResolvedType> {
    let mut members: Vec<PhpType> = Vec::new();
    for rt in held {
        for member in rt.type_string.union_members() {
            let member = member.non_null_type().unwrap_or_else(|| member.clone());
            if !member.is_object_like() {
                continue;
            }
            let fetched = if member.is_object() || member.is_object_shape() {
                PhpType::class_string(None)
            } else {
                PhpType::class_string(Some(member))
            };
            if !members.contains(&fetched) {
                members.push(fetched);
            }
        }
    }
    if members.is_empty() {
        return vec![];
    }
    vec![ResolvedType::from_type_string(PhpType::union(members))]
}

/// Try to resolve `$this->propName` from a prior assignment in the
/// current method body.
///
/// Walks the parsed AST to find the enclosing method, then scans its
/// statements for the last unconditional `$this->propName = <expr>`
/// before the cursor.  If found, resolves `<expr>` and returns the
/// result.  Returns an empty vec when no assignment is found (caller
/// should fall back to the declared property type).
pub(super) fn try_resolve_this_property_from_assignment(
    prop_name: &str,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    // Self-referencing assignments such as
    //   $this->prop = f($this->prop, ...);
    // read the same property on their own RHS.  Resolving that inner
    // read re-enters this function, whose "last assignment before the
    // cursor" search still finds the *same* assignment (the inner read
    // sits textually after the statement's start), producing unbounded
    // recursion.  Guard against re-entry on the same property with a
    // keyed visited-set: when the same `(class, prop)` is already being
    // resolved, return empty so the caller falls back to the declared
    // property type instead of recursing.
    thread_local! {
        static RESOLVING_THIS_PROP: std::cell::RefCell<HashSet<(Atom, Atom)>> =
            std::cell::RefCell::new(HashSet::new());
    }
    // Key on `(class, prop)` as interned atoms rather than a joined
    // `"Class::prop"` string: both halves are already interned, so the
    // tuple key allocates nothing per lookup and adds no new entries to
    // the global interner (the joined string would leak one entry per
    // distinct pair for the process lifetime).
    let key = (ctx.current_class.name, atom(prop_name));
    let newly_inserted = RESOLVING_THIS_PROP.with(|set| set.borrow_mut().insert(key));
    if !newly_inserted {
        // Same property is already mid-resolution: break the cycle.
        return Vec::new();
    }

    let result = with_parsed_program(
        ctx.file.content,
        "try_resolve_this_property_from_assignment",
        |program, _content| {
            // Find the RHS of the last `$this->propName = <expr>` in the
            // enclosing method body, before the cursor.
            let rhs_expr = find_this_property_assignment_in_toplevel(
                program.statements.iter(),
                prop_name,
                ctx.cursor_offset,
            );
            let Some(rhs_expr) = rhs_expr else {
                return Vec::new();
            };

            // Resolve the RHS expression with cursor set to the
            // assignment position so recursive resolution only sees
            // prior assignments.
            let rhs_ctx = ctx.with_cursor_offset(rhs_expr.span().start.offset);
            resolve_rhs_expression(rhs_expr, &rhs_ctx)
        },
    );

    RESOLVING_THIS_PROP.with(|set| {
        set.borrow_mut().remove(&key);
    });
    result
}

/// Search class-like members for a concrete method body containing `cursor_offset`,
/// then scan that body for the last `$this->propName = <expr>` assignment.
pub(super) fn find_property_assignment_in_members<'b>(
    members: impl Iterator<Item = &'b ClassLikeMember<'b>>,
    prop_name: &str,
    cursor_offset: u32,
) -> Option<&'b Expression<'b>> {
    let block = crate::util::find_enclosing_method_block_in_members(members, cursor_offset)?;
    find_last_this_property_assignment(block.statements.iter(), prop_name, cursor_offset)
}

/// Walk top-level statements to find the enclosing method, then scan
/// its body for the last `$this->propName = <expr>` before the cursor.
pub(super) fn find_this_property_assignment_in_toplevel<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    prop_name: &str,
    cursor_offset: u32,
) -> Option<&'b Expression<'b>> {
    for stmt in statements {
        let stmt_span = stmt.span();
        if cursor_offset < stmt_span.start.offset || cursor_offset > stmt_span.end.offset {
            continue;
        }
        match stmt {
            _ if let Some(body) = crate::parser::class_like_body(stmt) => {
                if let Some(found) = find_property_assignment_in_members(
                    body.members.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            // A class (and thus the enclosing method) can be declared
            // inside a namespace block or a conditional.  The recursive
            // call's cursor-containment check skips branches that don't
            // enclose the cursor.
            _ => {
                if let Some(found) = crate::parser::find_map_nested_statement(stmt, |inner| {
                    find_this_property_assignment_in_toplevel(
                        std::iter::once(inner),
                        prop_name,
                        cursor_offset,
                    )
                }) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Scan `statements` for the last unconditional `$this->propName = <expr>`
/// whose offset is before `cursor_offset`.  Returns the RHS expression.
pub(super) fn find_last_this_property_assignment<'b>(
    statements: impl Iterator<Item = &'b Statement<'b>>,
    prop_name: &str,
    cursor_offset: u32,
) -> Option<&'b Expression<'b>> {
    let mut last_rhs: Option<&'b Expression<'b>> = None;

    for stmt in statements {
        if stmt.span().start.offset >= cursor_offset {
            break;
        }
        if let Statement::Expression(expr_stmt) = stmt
            && let Some(rhs) = extract_this_property_assignment_rhs(expr_stmt.expression, prop_name)
        {
            last_rhs = Some(rhs);
        }
    }

    last_rhs
}

/// If `expr` is `$this->propName = <rhs>`, return `Some(rhs)`.
pub(super) fn extract_this_property_assignment_rhs<'b>(
    expr: &'b Expression<'b>,
    prop_name: &str,
) -> Option<&'b Expression<'b>> {
    let Expression::Assignment(assignment) = expr else {
        return None;
    };
    if !assignment.operator.is_assign() {
        return None;
    }
    let Expression::Access(Access::Property(pa)) = assignment.lhs else {
        return None;
    };
    let Expression::Variable(Variable::Direct(dv)) = pa.object else {
        return None;
    };
    if dv.name != b"$this" {
        return None;
    }
    let ClassLikeMemberSelector::Identifier(ident) = &pa.property else {
        return None;
    };
    if bytes_to_str(ident.value) != prop_name {
        return None;
    }
    Some(assignment.rhs)
}
