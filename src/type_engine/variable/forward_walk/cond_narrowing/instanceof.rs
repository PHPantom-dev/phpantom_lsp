use super::*;

/// Narrow every subject the `&&` chain's operands prove an
/// `instanceof`-style check about, and write the result to the scope.
///
/// Collecting across all operands before committing any of them is what
/// keeps a later operand from overwriting an earlier one when both narrow
/// the same subject: `$x instanceof Foo && $x instanceof Bar` has to end
/// up as `Foo&Bar`, not as whichever operand was looked at last.
///
/// Returns the subjects an operand pinned to a definite class, which is
/// what [`apply_disjunct_operand_narrowing`] reads to know whose type a
/// disjunction further along the chain must not widen back.
pub(super) fn commit_chain_instanceof<'b>(
    operands: &[&'b Expression<'b>],
    alias_extractions: &[Vec<AliasExtraction>],
    var_names: &[String],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<String> {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };

    // Track which variables have been narrowed by instanceof across
    // `&&` operands so we can merge them, plus where each subject's
    // classes came from so the merge knows whether they are alternatives
    // or an intersection.
    //
    // Operands that prove a single class and operands that prove a set of
    // alternatives are kept apart, because `&&` intersects what its
    // operands prove and these two cannot be intersected by unioning them
    // into one list.  `$b instanceof A && ($cls === A::class || $b
    // instanceof B)` proves `$b` is an `A`; merging `B` in as a peer would
    // answer `A|B` and lose the very thing the first operand established.
    let mut instanceof_results: HashMap<String, Vec<ResolvedType>> = HashMap::new();
    let mut alternative_results: HashMap<String, Vec<ResolvedType>> = HashMap::new();
    let mut conjuncts: HashMap<String, Conjuncts> = HashMap::new();

    for (op_idx, operand) in operands.iter().enumerate() {
        for var_name in var_names {
            // Compound OR instanceof: `$x instanceof A || $x instanceof B`
            if let Some(classes) = narrowing::try_extract_compound_or_instanceof(operand, var_name)
                && !classes.is_empty()
            {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                let union = narrowing::resolve_class_names_to_union(&classes, &var_ctx);
                if !union.is_empty() {
                    let entry = alternative_results.entry(var_name.clone()).or_default();
                    ResolvedType::extend_unique(
                        entry,
                        union.into_iter().map(ResolvedType::from_class).collect(),
                    );
                }
                continue;
            }

            // The same disjunction reached through a boolean that stands
            // for it (`$isNode = $n instanceof Stmt || $n instanceof Expr;
            // if ($isNode)`).
            if let Some(alias) = alias_extractions[op_idx]
                .iter()
                .find(|a| a.subject == *var_name && !a.alternatives.is_empty())
            {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                let classes = alias_classes(alias);
                if alias.extraction.negated {
                    // `!$isNode` — no leg of the chain held, so every one
                    // of them is excluded.
                    let mut results = scope.get(var_name).to_vec();
                    for cls in &classes {
                        ResolvedType::apply_narrowing(&mut results, |class_list| {
                            narrowing::apply_instanceof_exclusion(cls, &var_ctx, class_list)
                        });
                        scope.record_exclusion(var_name, cls);
                    }
                    if !results.is_empty() {
                        scope.set(var_name, results);
                    }
                } else {
                    let union = narrowing::resolve_class_names_to_union(&classes, &var_ctx);
                    if !union.is_empty() {
                        let entry = alternative_results.entry(var_name.clone()).or_default();
                        ResolvedType::extend_unique(
                            entry,
                            union.into_iter().map(ResolvedType::from_class).collect(),
                        );
                    }
                }
                continue;
            }

            // `$x instanceof $stmtClass` — the right-hand side is a value
            // holding the class name, so what it narrows to is whatever
            // that value's type says the class is.
            if let Some((rhs, negated)) =
                narrowing::try_extract_dynamic_instanceof(operand, var_name)
            {
                let targets = dynamic_instanceof_targets(rhs, scope, ctx);
                if !targets.is_empty() {
                    let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                    if negated {
                        let mut results = scope.get(var_name).to_vec();
                        for target in &targets {
                            ResolvedType::apply_narrowing(&mut results, |classes| {
                                narrowing::apply_instanceof_exclusion(target, &var_ctx, classes)
                            });
                            scope.record_exclusion(var_name, target);
                        }
                        if !results.is_empty() {
                            scope.set(var_name, results);
                        }
                    } else {
                        let mut resolved = Vec::new();
                        for target in &targets {
                            let mut single = Vec::new();
                            ResolvedType::apply_narrowing(&mut single, |classes| {
                                narrowing::apply_instanceof_inclusion(
                                    target, false, &var_ctx, classes,
                                )
                            });
                            ResolvedType::extend_unique(&mut resolved, single);
                        }
                        // An operand that names no loadable class (an
                        // unsubstituted `@template T` is the common one)
                        // proves nothing, so the subject is left as it
                        // was rather than emptied.
                        if !resolved.is_empty() {
                            if targets.len() > 1 {
                                ResolvedType::extend_unique(
                                    alternative_results.entry(var_name.clone()).or_default(),
                                    resolved,
                                );
                            } else {
                                ResolvedType::extend_unique(
                                    instanceof_results.entry(var_name.clone()).or_default(),
                                    resolved,
                                );
                                conjuncts.entry(var_name.clone()).or_default().operands += 1;
                            }
                        }
                    }
                    continue;
                }
            }

            // Single instanceof (including negated, is_a, get_class),
            // or a boolean that stands for one.
            if let Some(extraction) =
                narrowing::try_extract_instanceof_with_negation(operand, var_name).or_else(|| {
                    alias_extractions[op_idx]
                        .iter()
                        .find(|a| a.subject == *var_name)
                        .map(|a| a.extraction.clone())
                })
            {
                let var_ctx = build_var_ctx(var_name, ctx, &scope_resolver);
                if extraction.negated {
                    // Negated instanceof: apply exclusion to the current
                    // scope immediately (each negation removes one type).
                    let mut results = scope.get(var_name).to_vec();
                    ResolvedType::apply_narrowing(&mut results, |classes| {
                        narrowing::apply_instanceof_exclusion(
                            &extraction.class_type,
                            &var_ctx,
                            classes,
                        )
                    });
                    scope.record_exclusion(var_name, &extraction.class_type);
                    // Negated instanceof exclusion does NOT eliminate
                    // null — `!$x instanceof Foo` is true when $x is
                    // null, so null stays in the union.  No stripping.
                    if !results.is_empty() {
                        scope.set(var_name, results);
                    }
                } else {
                    // Positive instanceof: resolve and accumulate into
                    // the per-variable union.  For a single operand this
                    // produces `[Foo]`; for `&& instanceof Bar` it
                    // accumulates `[Foo, Bar]`.
                    let mut single = Vec::new();
                    ResolvedType::apply_narrowing(&mut single, |classes| {
                        narrowing::apply_instanceof_inclusion(
                            &extraction.class_type,
                            extraction.exact,
                            &var_ctx,
                            classes,
                        )
                    });
                    if !single.is_empty() {
                        let entry = instanceof_results.entry(var_name.clone()).or_default();
                        ResolvedType::extend_unique(entry, single);
                        let c = conjuncts.entry(var_name.clone()).or_default();
                        c.operands += 1;
                        c.allow_string |= extraction.allow_string;
                        c.exact |= extraction.exact;
                    } else {
                        // Target class is unresolvable — mark variable
                        // as empty so diagnostics suppress false positives.
                        instanceof_results.entry(var_name.clone()).or_default();
                    }
                }
            }
        }
    }

    // A subject an operand proved a definite class for keeps that class:
    // every alternative the rest of the chain allows is one the definite
    // check already has to admit, so the definite class bounds them all
    // and is the safe answer.  Alternatives narrow the subject only when
    // nothing in the chain pinned it down.
    for (var_name, alternatives) in alternative_results {
        let entry = instanceof_results.entry(var_name).or_default();
        if entry.is_empty() {
            *entry = alternatives;
        }
    }

    // Apply the accumulated instanceof narrowing results to the scope.
    for (var_name, narrowed) in instanceof_results {
        // `$x instanceof A && $x instanceof B` proves both at once, so the
        // classes gathered across the operands are members of `A&B`
        // rather than alternatives a consumer may pick one of.
        let shape = CheckShape {
            intersected: conjuncts
                .get(&var_name)
                .is_some_and(Conjuncts::is_intersection),
            allow_string: conjuncts.get(&var_name).is_some_and(|c| c.allow_string),
            exact: conjuncts.get(&var_name).is_some_and(|c| c.exact),
        };
        commit_instanceof_narrowing(&var_name, narrowed, shape, scope, ctx, &scope_resolver);
    }

    conjuncts.into_keys().collect()
}

/// Write the outcome of a *successful* `instanceof` check on `var_name`
/// into the scope.
///
/// `narrowed` holds the classes the check proves the value is; the
/// variable's current types decide how they combine with what was already
/// known.  Both polarities of the check reach this: a positive
/// `instanceof` in a truthy branch, and the fall-through of a
/// `if (!$x instanceof T) { return; }` guard, which proves exactly the
/// same thing.  Keeping one implementation is what stops the guard form
/// from drifting back into *adding* `T` to the union instead of
/// filtering it down to `T`.
/// The classes `$subject instanceof <value>` narrows to, when the
/// right-hand side is a value rather than a literal class name.
///
/// A `class-string<T>` names `T`; a union of them names each `T` as an
/// alternative; and an object-typed right-hand side stands for its own
/// class, which is what `$node instanceof $other` checks.  A bare
/// `class-string` or plain `string` names nothing, so the check proves
/// nothing and the subject is left alone.
pub(super) fn dynamic_instanceof_targets(
    rhs: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Vec<PhpType> {
    let scope_snapshot = scope.locals.clone();
    let scope_resolver = |vn: &str| -> Vec<ResolvedType> {
        scope_snapshot.get(&atom(vn)).cloned().unwrap_or_default()
    };
    let var_ctx = build_var_ctx("", ctx, &scope_resolver);
    let Some(resolved) =
        crate::type_engine::variable::resolution::resolve_arg_raw_type(rhs, &var_ctx)
    else {
        return Vec::new();
    };

    let mut targets = Vec::new();
    for member in resolved.union_members() {
        let target = match member.kind() {
            TypeKind::ClassString(Some(inner)) | TypeKind::InterfaceString(Some(inner)) => {
                inner.clone()
            }
            // An object-typed operand stands for its own class.  A
            // keyword type (`string`, `object`, `mixed`) names no class,
            // so the check proves nothing about the subject.
            TypeKind::Named(_) | TypeKind::Generic { .. } if !member.is_keyword() => member.clone(),
            _ => continue,
        };
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

pub(super) fn commit_instanceof_narrowing(
    var_name: &str,
    mut narrowed: Vec<ResolvedType>,
    shape: CheckShape,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    scope_resolver: &dyn Fn(&str) -> Vec<ResolvedType>,
) {
    let CheckShape {
        intersected,
        allow_string,
        exact,
    } = shape;
    if intersected {
        ResolvedType::tag_as_intersection(&mut narrowed);
    }
    if narrowed.is_empty() {
        // Empty narrowed list means the target was unresolvable.
        scope.set_untyped(var_name);
        return;
    }

    let existing = scope.get(var_name);
    if existing.is_empty() {
        // Untyped variable — instanceof provides the type.
        scope.set(var_name, narrowed);
        return;
    }

    // `is_a($x, C::class, true)` also passes when `$x` is a
    // `class-string<C>`, so a string alternative already in the subject's
    // type must survive every path below that would otherwise replace the
    // whole union with just the checked class. The string half can be a
    // whole entry (`string`) or one member of a broader union carried by a
    // single entry (`object|string`), so pull it out at the union-member
    // level rather than requiring the whole entry to be string-only.
    // `apply_class_string_guard_narrowing` (run right after this pass)
    // turns the preserved string alternative into `class-string<C>`.
    let string_alt: Vec<ResolvedType> = if allow_string {
        existing
            .iter()
            .filter(|rt| rt.class_info.is_none())
            .filter_map(|rt| {
                let stringy: Vec<PhpType> = rt
                    .type_string
                    .union_members()
                    .into_iter()
                    .filter(|m| m.is_subtype_of(&PhpType::string()))
                    .cloned()
                    .collect();
                match stringy.len() {
                    0 => None,
                    1 => Some(ResolvedType::from_type_string(
                        stringy.into_iter().next().unwrap(),
                    )),
                    _ => Some(ResolvedType::from_type_string(PhpType::union(stringy))),
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let with_string_alt = |mut result: Vec<ResolvedType>| -> Vec<ResolvedType> {
        if !string_alt.is_empty() {
            ResolvedType::extend_unique(&mut result, string_alt.clone());
        }
        result
    };

    // A successful check rules out every alternative that cannot be an
    // object, and an `object`/`mixed` alternative says nothing the checked
    // class does not already say.  When that leaves no class alternative
    // to filter down to, the check's conclusion is the whole answer:
    // `object|string` (the shape a route parameter arrives as) becomes the
    // checked class instead of growing it as one more alternative.  `null`
    // counts as ruled out too, so `?object` is as uninformative as bare
    // `object`.
    //
    // A subject with no object alternative at all is deliberately left to
    // the passes that run after this one: `is_a($s, C::class, true)` on a
    // `string` proves a `class-string<C>`, not a `C`.
    let is_broad_object = |ty: &PhpType| {
        matches!(
            ty.kind(),
            TypeKind::Named(n) if n.eq_ignore_ascii_case("mixed") || n.eq_ignore_ascii_case("object")
        )
    };
    let mut names_class = false;
    let mut broad_object = false;
    for rt in existing {
        if rt.class_info.is_some() {
            names_class = true;
            break;
        }
        for member in rt.type_string.union_members() {
            if !member.top_level_class_names().is_empty() {
                names_class = true;
                break;
            }
            let bare = member.non_null_type();
            if is_broad_object(bare.as_ref().unwrap_or(member)) {
                broad_object = true;
            }
        }
        if names_class {
            break;
        }
    }
    if !names_class && broad_object {
        scope.set(var_name, with_string_alt(narrowed));
        return;
    }

    // The other half of that rule: a subject that can only hold a string
    // has no object alternative for the checked class to filter down to,
    // so `is_a($s, C::class, true)` proves a `class-string<C>` and
    // nothing more.  Leave it to `apply_class_string_guard_narrowing`,
    // which runs right after this pass — adding `C` here would put an
    // object alternative into a value that is definitely a string.
    if allow_string && !string_alt.is_empty() && !names_class {
        return;
    }

    // Typed variable — filter the existing union to only types present in
    // the narrowed set.  This correctly handles both single instanceof
    // (`Dog|Cat` → `Dog`) and OR instanceof (`Dog|Cat|Other` → `Dog|Cat`).
    //
    // When the narrowed type is NOT in the existing union (e.g.
    // `MockInterface` narrowed to `MolliePayment`), this is an
    // intersection case — apply via apply_instanceof_inclusion which has
    // interface intersection logic.
    let narrowed_fqns: Vec<String> = narrowed
        .iter()
        .filter_map(|rt| rt.class_info.as_ref().map(|c| c.fqn().to_string()))
        .collect();

    // Try filtering: keep existing entries whose class is in the narrowed
    // set, or is a subtype of one — a `VarLikeIdentifier` alternative
    // passes `instanceof Identifier` and stays as itself, being the more
    // specific of the two.  An exact check
    // (`get_class($x) === Identifier::class`) pins the identity instead,
    // and there the subclass does not pass.
    //
    // A kept entry's own type_string may still be the whole pre-check union
    // (a conditional return type resolves to one entry naming a class and
    // listing an array alternative beside it), so restrict it to the
    // surviving classes as well.  Strip null on top because a successful
    // instanceof check guarantees the value is non-null (`?Foo` → `Foo`).
    let passes_check = |fqn: &str| {
        narrowed_fqns.iter().any(|n| {
            n == fqn
                || (!exact && crate::class_lookup::is_subtype_of_names(fqn, n, ctx.class_loader))
        })
    };
    let survives = |name: &str| {
        narrowed.iter().any(|rt| {
            rt.class_info
                .as_ref()
                .is_some_and(|c| c.name == name || c.fqn() == name)
        }) || passes_check(name)
    };
    let filtered: Vec<ResolvedType> = existing
        .iter()
        .filter(|rt| {
            rt.class_info
                .as_ref()
                .is_some_and(|c| passes_check(&c.fqn()))
        })
        .map(|rt| {
            let mut rt = rt.clone();
            rt.restrict_type_string_to_classes(&survives);
            if let Some(non_null) = rt.type_string.non_null_type() {
                rt.type_string = non_null;
            }
            rt
        })
        .collect();

    if !filtered.is_empty() {
        // Filter matched — use the filtered results (preserves richer type
        // info from original resolution).  Also strip bare `null` entries:
        // a successful instanceof check guarantees non-null, so `null`
        // entries added by `from_classes_with_hint` must be removed.
        let mut filtered: Vec<ResolvedType> = filtered
            .into_iter()
            .filter(|rt| !rt.type_string.is_null())
            .collect();
        if intersected {
            ResolvedType::tag_as_intersection(&mut filtered);
        }
        if filtered.is_empty() {
            scope.set(var_name, with_string_alt(narrowed));
        } else {
            scope.set(var_name, with_string_alt(filtered));
        }
        return;
    }

    // No overlap between existing and narrowed types.  This is the
    // intersection case (e.g. MockInterface narrowed to MolliePayment).
    // Use apply_instanceof_inclusion which produces the intersection when
    // one side is an interface.
    let mut results = existing.to_vec();
    // Apply all narrowed classes as a single group by building a union type.
    let union_type = if narrowed_fqns.len() == 1 {
        PhpType::named(atom(&narrowed_fqns[0]))
    } else {
        PhpType::union(
            narrowed_fqns
                .iter()
                .map(|n| PhpType::named(atom(n)))
                .collect(),
        )
    };
    let var_ctx = build_var_ctx(var_name, ctx, scope_resolver);
    ResolvedType::apply_narrowing(&mut results, |classes| {
        narrowing::apply_instanceof_inclusion(&union_type, false, &var_ctx, classes)
    });
    // Instanceof guarantees non-null — strip bare `null` entries that were
    // preserved by `apply_narrowing`'s `None => true` rule.
    results.retain(|rt| !rt.type_string.is_null());
    // `apply_instanceof_inclusion` merging in an unrelated interface (the
    // branch this call site exists for) leaves both classes in `results` as
    // separate entries, which describe one value that is both at once —
    // recognisable by a class surviving that the check never named.  When
    // the inclusion instead *replaced* the subject's class (the checked
    // classes are the more specific ones), the result is those classes as
    // the check handed them over, and a `||` chain hands over
    // alternatives, not an intersection.
    let kept_unchecked_class = results.iter().any(|rt| {
        rt.class_info
            .as_ref()
            .is_some_and(|c| !narrowed_fqns.iter().any(|n| n == c.fqn()))
    });
    if intersected || kept_unchecked_class {
        ResolvedType::tag_as_intersection(&mut results);
    }
    if !results.is_empty() {
        scope.set(var_name, with_string_alt(results));
    } else {
        // Fallback: use the narrowed types directly.
        scope.set(var_name, with_string_alt(narrowed));
    }
}
