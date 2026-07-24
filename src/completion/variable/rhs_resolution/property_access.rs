/// Property access (`$this->prop`, `$obj->prop`, `$obj?->prop`)
/// resolution: resolves the property's type hint, plus the
/// `find_*_this_property_assignment*` scanners used to recover a
/// property's type from its last constructor/method assignment when no
/// declared type is available.
use std::collections::HashSet;
use std::sync::Arc;

use mago_span::HasSpan;
use mago_syntax::cst::*;

use crate::atom::bytes_to_str;
use crate::parser::with_parsed_program;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, ResolvedType};

use crate::completion::resolver::VarResolutionCtx;

use super::{infer_type_from_constant_value, resolve_rhs_expression, resolved_type_with_lookup};

/// Resolve property access: `$this->prop`, `$obj->prop`, `$obj?->prop`.
pub(super) fn resolve_rhs_property_access(
    access: &Access<'_>,
    ctx: &VarResolutionCtx<'_>,
) -> Vec<ResolvedType> {
    let current_class_name: &str = &ctx.current_class.name;
    let all_classes = ctx.all_classes;
    let class_loader = ctx.class_loader;

    /// Resolve a property's type to `Vec<ResolvedType>`, preserving the
    /// property's type hint string in each result.
    ///
    /// When the property type is a scalar (e.g. `string`, `int`) and
    /// `type_hint_to_classes_typed` returns no `ClassInfo`, a type-string-only
    /// `ResolvedType` is produced so that the type information is not lost.
    fn resolve_property_with_hint(
        prop_name: &str,
        owner: &ClassInfo,
        current_class_name: &str,
        all_classes: &[Arc<ClassInfo>],
        class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    ) -> Vec<ResolvedType> {
        // Get the type hint before resolving to ClassInfo.
        let type_hint =
            crate::inheritance::resolve_property_type_hint(owner, prop_name, class_loader);
        let resolved = crate::completion::type_resolution::resolve_property_types(
            prop_name,
            owner,
            all_classes,
            class_loader,
        );
        if resolved.is_empty() {
            // The property has a type hint but `type_hint_to_classes_typed`
            // found no matching class (e.g. `list<Widget>`, `int`,
            // `array{name: string}`).  Return a type-string-only
            // entry when the type is informative (carries generics,
            // shapes, or names a non-scalar class).
            return match type_hint {
                Some(hint) => {
                    vec![resolved_type_with_lookup(
                        hint,
                        current_class_name,
                        all_classes,
                        class_loader,
                    )]
                }
                _ => vec![],
            };
        }
        match type_hint {
            Some(hint) => ResolvedType::from_classes_with_hint(resolved, hint),
            None => ResolvedType::from_classes(resolved),
        }
    }

    // ── Class constant / enum case access: `Foo::BAR` ──
    // When the RHS is a class constant access, resolve the class and
    // check whether the constant is an enum case (→ type is the enum
    // itself) or a typed constant (→ use its type_hint).
    if let Access::ClassConstant(cca) = access {
        let class_name = match cca.class {
            Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
            Expression::Self_(_) => Some(current_class_name.to_string()),
            Expression::Static(_) => Some(current_class_name.to_string()),
            _ => None,
        };
        if let Some(class_name) = class_name {
            let resolved_name = class_name.strip_prefix('\\').unwrap_or(&class_name);
            let resolved_typed = PhpType::Named(resolved_name.to_string());
            let target_classes = crate::completion::type_resolution::type_hint_to_classes_typed(
                &resolved_typed,
                current_class_name,
                all_classes,
                class_loader,
            );

            let const_name = match &cca.constant {
                ClassLikeConstantSelector::Identifier(ident) => {
                    Some(bytes_to_str(ident.value).to_string())
                }
                _ => None,
            };

            // The magic `::class` constant yields the fully-qualified name
            // of the class as a `class-string<T>`. Resolving it to a plain
            // `string` would discard the class identity, so downstream
            // consumers (array element inference, `??` fallbacks, and
            // `class-string<object>` parameters) keep the concrete class.
            if const_name.as_deref() == Some("class") {
                return vec![ResolvedType::from_type_string(PhpType::ClassString(Some(
                    Box::new(PhpType::Named(resolved_name.to_string())),
                )))];
            }

            if let Some(const_name) = const_name {
                // Search local classes first.  If the constant is not
                // found, resolve via full inheritance merging so that
                // constants from parent classes are visible (e.g.
                // `self::PARENT_CONST` in a subclass).
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
                    // Check if the constant is an enum case — the
                    // result type is the enum class itself.
                    if let Some(c) = cls.constants.iter().find(|c| c.name == const_name) {
                        if c.is_enum_case {
                            return ResolvedType::from_classes(target_classes);
                        }
                        // Typed class constant — resolve via type_hint.
                        if let Some(ref th) = c.type_hint {
                            let resolved =
                                crate::completion::type_resolution::type_hint_to_classes_typed(
                                    th,
                                    current_class_name,
                                    all_classes,
                                    class_loader,
                                );
                            if !resolved.is_empty() {
                                return ResolvedType::from_classes_with_hint(resolved, th.clone());
                            }
                        }
                        // No type_hint — infer from the initializer value.
                        if let Some(ref val) = c.value
                            && let Some(ts) = infer_type_from_constant_value(val)
                        {
                            let resolved =
                                crate::completion::type_resolution::type_hint_to_classes_typed(
                                    &ts,
                                    current_class_name,
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
            }
        }
        return vec![];
    }

    // ── Static property access: `self::$prop`, `static::$prop`, `Foo::$prop` ──
    if let Access::StaticProperty(spa) = access {
        let class_name = match spa.class {
            Expression::Identifier(ident) => Some(bytes_to_str(ident.value()).to_string()),
            Expression::Self_(_) => Some(current_class_name.to_string()),
            Expression::Static(_) => Some(current_class_name.to_string()),
            Expression::Parent(_) => {
                // Resolve parent class name from the current class.
                all_classes
                    .iter()
                    .find(|c| c.name == current_class_name)
                    .and_then(|c| c.parent_class.map(|a| a.to_string()))
            }
            _ => None,
        };
        let prop_name = match &spa.property {
            Variable::Direct(dv) => {
                let raw = bytes_to_str(dv.name).to_string();
                Some(raw.strip_prefix('$').unwrap_or(&raw).to_string())
            }
            _ => None,
        };
        if let Some(class_name) = class_name
            && let Some(prop_name) = prop_name
        {
            let resolved_name = class_name.strip_prefix('\\').unwrap_or(&class_name);
            let resolved_typed = PhpType::Named(resolved_name.to_string());
            let target_classes = crate::completion::type_resolution::type_hint_to_classes_typed(
                &resolved_typed,
                current_class_name,
                all_classes,
                class_loader,
            );
            for cls in &target_classes {
                let resolved = resolve_property_with_hint(
                    &prop_name,
                    cls,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                if !resolved.is_empty() {
                    return resolved;
                }
            }
        }
        return vec![];
    }

    let (object_expr, prop_selector) = match access {
        Access::Property(pa) => (Some(pa.object), Some(&pa.property)),
        Access::NullSafeProperty(pa) => (Some(pa.object), Some(&pa.property)),
        _ => (None, None),
    };
    if let Some(obj) = object_expr
        && let Some(sel) = prop_selector
    {
        let prop_name = match sel {
            ClassLikeMemberSelector::Identifier(ident) => {
                Some(bytes_to_str(ident.value).to_string())
            }
            _ => None,
        };
        if let Some(prop_name) = prop_name {
            // ── $this->prop assignment narrowing ────────────────
            // When the object is `$this`, check if there is an
            // assignment to `$this->propName` before the cursor in
            // the current method.  If so, use the assigned value's
            // type — but ONLY when it is narrower (a subtype of)
            // the declared property type.  This handles patterns
            // like:
            //   $this->mock = $this->createMock(Foo::class);
            //   new Bar($this->mock); // mock is MockObject&Foo
            //
            // We reject widening assignments (e.g. narrowed type is
            // `object` but declared type is `Foo`) to avoid losing
            // declared type information.
            if let Expression::Variable(Variable::Direct(dv)) = obj
                && dv.name == b"$this"
            {
                let narrowed = try_resolve_this_property_from_assignment(&prop_name, ctx);
                if !narrowed.is_empty() {
                    // Look up the declared property type so we can
                    // verify the narrowed type is actually narrower.
                    let current_class_arc =
                        all_classes.iter().find(|c| c.name == current_class_name);
                    let declared_type = current_class_arc.and_then(|cls| {
                        crate::inheritance::resolve_property_type_hint(
                            cls,
                            &prop_name,
                            class_loader,
                        )
                    });
                    if let Some(ref declared) = declared_type {
                        // Only use the narrowed type when every
                        // resolved type is a subtype of the declared
                        // type.  Use structural subtyping first, then
                        // fall back to nominal class hierarchy.
                        let all_narrow = narrowed.iter().all(|rt| {
                            let ts = &rt.type_string;
                            // Structural check covers scalars, unions,
                            // intersections, nullable, generic, etc.
                            if ts.is_subtype_of(declared) {
                                return true;
                            }
                            // Nominal check: if both are class-like,
                            // walk the class hierarchy.
                            if let Some(narrowed_base) = ts.base_name()
                                && let Some(cls) = (class_loader)(narrowed_base)
                                && let Some(declared_base) = declared.base_name()
                            {
                                return crate::class_lookup::is_subtype_of(
                                    &cls,
                                    declared_base,
                                    class_loader,
                                );
                            }
                            // Intersection types: each member must be
                            // a subtype.  If any member satisfies the
                            // declared type, the intersection does too.
                            if let crate::php_type::PhpType::Intersection(members) = ts {
                                return members.iter().any(|m| {
                                    if m.is_subtype_of(declared) {
                                        return true;
                                    }
                                    if let Some(base) = m.base_name()
                                        && let Some(cls) = (class_loader)(base)
                                        && let Some(declared_base) = declared.base_name()
                                    {
                                        return crate::class_lookup::is_subtype_of(
                                            &cls,
                                            declared_base,
                                            class_loader,
                                        );
                                    }
                                    false
                                });
                            }
                            false
                        });
                        if all_narrow {
                            return narrowed;
                        }
                        // Narrowed type is wider than declared — fall
                        // through to normal property type resolution.
                    } else {
                        // No declared type (untyped property) — the
                        // narrowed type is the best we have.
                        return narrowed;
                    }
                }
            }

            let owner_classes: Vec<Arc<ClassInfo>> =
                if let Expression::Variable(Variable::Direct(dv)) = obj
                    && dv.name == b"$this"
                {
                    all_classes
                        .iter()
                        .find(|c| c.name == current_class_name)
                        .map(Arc::clone)
                        .into_iter()
                        .collect()
                } else if let Expression::Variable(Variable::Direct(dv)) = obj {
                    let var = bytes_to_str(dv.name).to_string();
                    // Check match-arm narrowing override first.
                    if let Some(overridden) = ctx.match_arm_narrowing.get(&var).cloned() {
                        ResolvedType::into_arced_classes(overridden)
                    } else {
                        // When a scope_var_resolver is available (forward-walker
                        // RHS resolution), try it first so we read from the
                        // in-progress ScopeState instead of the diagnostic
                        // scope cache or backward scanner.
                        let from_scope = if let Some(resolver) = ctx.scope_var_resolver {
                            let prefixed = if var.starts_with('$') {
                                var.clone()
                            } else {
                                format!("${}", var)
                            };
                            resolver(&prefixed)
                        } else {
                            vec![]
                        };
                        let classes = ResolvedType::into_arced_classes(from_scope);
                        if !classes.is_empty() {
                            classes
                        } else {
                            ResolvedType::into_arced_classes(
                                crate::completion::resolver::resolve_target_classes(
                                    &var,
                                    crate::types::AccessKind::Arrow,
                                    &ctx.as_resolution_ctx(),
                                ),
                            )
                        }
                    }
                } else {
                    // Handle non-variable object expressions like
                    // `(new Canvas())->easel`, `getService()->prop`,
                    // or `SomeClass::make()->prop` by recursively
                    // resolving the expression type.
                    ResolvedType::into_arced_classes(resolve_rhs_expression(obj, ctx))
                };

            let mut all_resolved: Vec<ResolvedType> = Vec::new();
            for owner in &owner_classes {
                let resolved = resolve_property_with_hint(
                    &prop_name,
                    owner,
                    current_class_name,
                    all_classes,
                    class_loader,
                );
                for rt in resolved {
                    if !all_resolved
                        .iter()
                        .any(|existing| existing.type_string == rt.type_string)
                    {
                        all_resolved.push(rt);
                    }
                }
            }
            if !all_resolved.is_empty() {
                return all_resolved;
            }
        }
    }
    vec![]
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
        static RESOLVING_THIS_PROP: std::cell::RefCell<HashSet<String>> =
            std::cell::RefCell::new(HashSet::new());
    }
    let key = format!("{}::{}", ctx.current_class.name, prop_name);
    let newly_inserted = RESOLVING_THIS_PROP.with(|set| set.borrow_mut().insert(key.clone()));
    if !newly_inserted {
        // Same property is already mid-resolution: break the cycle.
        return Vec::new();
    }

    let result = with_parsed_program(
        ctx.content,
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
            Statement::Class(class) => {
                if let Some(found) = find_property_assignment_in_members(
                    class.members.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::Trait(trait_def) => {
                if let Some(found) = find_property_assignment_in_members(
                    trait_def.members.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::Enum(enum_def) => {
                if let Some(found) = find_property_assignment_in_members(
                    enum_def.members.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::Namespace(ns) => {
                if let Some(found) = find_this_property_assignment_in_toplevel(
                    ns.statements().iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            Statement::If(if_stmt) => {
                // Walk the then-branch, every elseif branch, and the else
                // branch: an assignment to `$this->prop` in any of them
                // (before the cursor) is a valid narrowing source.
                let search = |inner: &'b Statement<'b>| {
                    find_this_property_assignment_in_toplevel(
                        std::iter::once(inner),
                        prop_name,
                        cursor_offset,
                    )
                };
                match &if_stmt.body {
                    IfBody::Statement(body) => {
                        if let Some(found) = search(body.statement) {
                            return Some(found);
                        }
                        for elseif in body.else_if_clauses.iter() {
                            if let Some(found) = search(elseif.statement) {
                                return Some(found);
                            }
                        }
                        if let Some(ref else_clause) = body.else_clause
                            && let Some(found) = search(else_clause.statement)
                        {
                            return Some(found);
                        }
                    }
                    IfBody::ColonDelimited(body) => {
                        if let Some(found) = find_this_property_assignment_in_toplevel(
                            body.statements.iter(),
                            prop_name,
                            cursor_offset,
                        ) {
                            return Some(found);
                        }
                        for elseif in body.else_if_clauses.iter() {
                            if let Some(found) = find_this_property_assignment_in_toplevel(
                                elseif.statements.iter(),
                                prop_name,
                                cursor_offset,
                            ) {
                                return Some(found);
                            }
                        }
                        if let Some(ref else_clause) = body.else_clause
                            && let Some(found) = find_this_property_assignment_in_toplevel(
                                else_clause.statements.iter(),
                                prop_name,
                                cursor_offset,
                            )
                        {
                            return Some(found);
                        }
                    }
                }
            }
            Statement::Block(block) => {
                if let Some(found) = find_this_property_assignment_in_toplevel(
                    block.statements.iter(),
                    prop_name,
                    cursor_offset,
                ) {
                    return Some(found);
                }
            }
            _ => {}
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
