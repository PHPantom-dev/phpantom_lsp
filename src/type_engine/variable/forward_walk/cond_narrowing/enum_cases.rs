//! Narrowing a subject compared identical to an enum case.
//!
//! Each case of an enum is a type of its own, so `$suit === Suit::Hearts`
//! leaves `Suit::Hearts` in the branch where it holds and the enum's other
//! cases in the one where it does not. A branch that has compared every case
//! away holds nothing at all.

use super::*;

use super::in_array::values_held_by;
use crate::class_lookup::Fqn;
use crate::type_engine::call_resolution::enum_case_property_literal;
use crate::types::{ClassInfo, ClassLikeKind};

/// Apply what an identity comparison against an enum case proves about the
/// subject, in whichever direction the branch establishes.
///
/// `truthy` is the polarity the caller narrows for: `true` for the branch
/// where the written condition held, `false` for the one where it did not.
pub(super) fn apply_enum_case_identity_narrowing(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    for identical in [true, false] {
        let Some((var_name, constant)) = extract_class_constant_identity(condition, identical)
        else {
            continue;
        };
        let Some(case) = enum_case_comparand(constant, scope, ctx) else {
            return;
        };
        if identical == truthy {
            narrow_to_enum_cases(&var_name, vec![case], scope, ctx);
        } else {
            subtract_enum_cases(&var_name, &[case], scope, ctx);
        }
        return;
    }
}

/// Apply what comparing a backed enum's `->value` against literals proves
/// about the enum, in whichever direction the branch establishes.
///
/// A backed enum's cases and backing values map one to one, so
/// `$s->value === 'H'` proves `$s` is the case backed by `'H'` and
/// `$s->value !== 'H'` rules that case out. A strict
/// `in_array($s->value, [...], true)` does the same for each value the
/// haystack names.
pub(super) fn apply_enum_value_identity_narrowing(
    condition: &Expression<'_>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    if let Some((var_name, literal, equal)) = extract_backing_value_identity(condition) {
        narrow_by_backing_values(&var_name, &[literal], equal == truthy, scope, ctx);
        return;
    }
    let (inner, negated) = condition_facts::unwrap_condition_negation(condition);
    let Expression::Call(Call::Function(call)) = unwrap_parens(inner) else {
        return;
    };
    let Some(needle) = call.argument_list.arguments.first().map(|arg| arg.value()) else {
        return;
    };
    let Some(var_name) = backing_value_receiver(needle) else {
        return;
    };
    let Some(needle_key) = condition_facts::expr_to_subject_key(needle) else {
        return;
    };
    let Some(haystack_expr) = condition_facts::try_extract_in_array(inner, &needle_key) else {
        return;
    };
    let Some(haystack) = resolve_in_array_haystack_type_fw(haystack_expr, scope, ctx) else {
        return;
    };
    if truthy != negated {
        // The needle is one of the haystack's elements, which names the
        // cases only when every element is a value written out.
        let Some(element) = haystack.iterable_element_type() else {
            return;
        };
        let values: Vec<PhpType> = element.union_members().into_iter().cloned().collect();
        if values.iter().all(|value| value.as_literal().is_some()) {
            narrow_by_backing_values(&var_name, &values, true, scope, ctx);
        }
    } else {
        narrow_by_backing_values(&var_name, &values_held_by(&haystack), false, scope, ctx);
    }
}

/// The subject a strict comparison of `->value` against a literal reads
/// the value of, the literal, and whether the branch being narrowed is the
/// one where the two were identical.
fn extract_backing_value_identity(expr: &Expression<'_>) -> Option<(String, PhpType, bool)> {
    let (inner, negated) = condition_facts::unwrap_condition_negation(expr);
    let Expression::Binary(bin) = unwrap_parens(inner) else {
        return None;
    };
    let equal = match bin.operator {
        BinaryOperator::Identical(_) => !negated,
        BinaryOperator::NotIdentical(_) => negated,
        _ => return None,
    };
    for (candidate, other) in [(bin.rhs, bin.lhs), (bin.lhs, bin.rhs)] {
        if let Some(literal) = literal_comparand_type(candidate)
            && literal.as_literal().is_some()
            && let Some(var_name) = backing_value_receiver(other)
        {
            return Some((var_name, literal, equal));
        }
    }
    None
}

/// The subject `expr` reads `->value` (or `?->value`) of.
fn backing_value_receiver(expr: &Expression<'_>) -> Option<String> {
    let (object, property) = match unwrap_parens(expr) {
        Expression::Access(Access::Property(pa)) => (pa.object, &pa.property),
        Expression::Access(Access::NullSafeProperty(pa)) => (pa.object, &pa.property),
        _ => return None,
    };
    let ClassLikeMemberSelector::Identifier(ident) = property else {
        return None;
    };
    if bytes_to_str(ident.value) != "value" {
        return None;
    }
    expr_to_subject(object)
}

/// Narrow `var_name` by its `->value` being (`equal`) or not being one of
/// `values`.
///
/// The equal branch narrows only a subject that holds nothing but backed
/// enums (and `null`, whose `->value` is no value): any other object could
/// carry a `value` of its own. A case whose backing value cannot be read
/// stays in, since it may be the one compared against. A value no case is
/// backed by describes a branch that cannot run, which is not this
/// function's business to decide, so it leaves the subject alone.
fn narrow_by_backing_values(
    var_name: &str,
    values: &[PhpType],
    equal: bool,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    let literals: Vec<&LiteralValue> = values.iter().filter_map(PhpType::as_literal).collect();
    if literals.is_empty() {
        return;
    }
    seed_synthetic_key_if_needed(var_name, scope, ctx);
    let types = scope.get(var_name);
    if types.is_empty() {
        return;
    }
    let mut only_backed_enums = true;
    let mut matched: Vec<ResolvedType> = Vec::new();
    let mut possible: Vec<ResolvedType> = Vec::new();
    for rt in types {
        for member in rt.type_string.union_members() {
            let member = match member.kind() {
                TypeKind::Nullable(inner) => inner,
                _ => member,
            };
            if member.is_null() {
                continue;
            }
            let (enum_name, only_case) = match (member.as_enum_case(), member.kind()) {
                (Some((enum_name, case)), _) => (enum_name, Some(case)),
                (None, TypeKind::Named(name)) => (name.as_ref(), None),
                _ => {
                    only_backed_enums = false;
                    continue;
                }
            };
            let enum_class = match &rt.class_info {
                Some(cls) if cls.fqn().eq_ignore_ascii_case(enum_name) => Arc::clone(cls),
                _ => match (ctx.file.class_loader)(Fqn::new(enum_name)) {
                    Some(cls) => cls,
                    None => {
                        only_backed_enums = false;
                        continue;
                    }
                },
            };
            if enum_class.kind != ClassLikeKind::Enum || enum_class.backed_type.is_none() {
                only_backed_enums = false;
                continue;
            }
            let enum_fqn = enum_class.fqn();
            for constant in enum_class
                .constants
                .iter()
                .filter(|c| c.is_enum_case && only_case.is_none_or(|case| c.name == case))
            {
                let identical = enum_case_property_literal(&enum_class, &constant.name, "value")
                    .and_then(|backing| {
                        let backing = backing.as_literal()?.clone();
                        let answers: Vec<Option<bool>> = literals
                            .iter()
                            .map(|lit| backing.identical_to(lit))
                            .collect();
                        if answers.contains(&Some(true)) {
                            Some(true)
                        } else if answers.contains(&None) {
                            None
                        } else {
                            Some(false)
                        }
                    });
                let case_type = PhpType::enum_case(enum_fqn, constant.name);
                if identical == Some(false) || possible.iter().any(|rt| rt.type_string == case_type)
                {
                    continue;
                }
                let case = ResolvedType::from_both_arc(case_type, Arc::clone(&enum_class));
                if identical == Some(true) {
                    matched.push(case.clone());
                }
                possible.push(case);
            }
        }
    }
    if matched.is_empty() {
        return;
    }
    if !equal {
        subtract_enum_cases(var_name, &matched, scope, ctx);
    } else if only_backed_enums {
        narrow_to_enum_cases(var_name, possible, scope, ctx);
    }
}

/// The enum case `expr` reads, when it is written as a class constant
/// naming one: `Suit::Hearts`, or a constant initialised to a case.
pub(crate) fn enum_case_comparand(
    expr: &Expression<'_>,
    scope: &ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) -> Option<ResolvedType> {
    if !matches!(
        unwrap_parens(expr),
        Expression::Access(Access::ClassConstant(_))
    ) {
        return None;
    }
    let resolved = resolve_rhs_with_scope(expr, scope, ctx);
    let [rt] = resolved.as_slice() else {
        return None;
    };
    (rt.type_string.as_enum_case().is_some() && rt.class_info.is_some()).then(|| rt.clone())
}

/// Narrow `var_name` to the enum cases it was proven to be one of.
///
/// Only a subject whose type has room for one of them is narrowed: a
/// comparison no alternative could satisfy describes a branch that cannot
/// run, which is not this function's business to decide. An unknown
/// subject admits every value, so the comparison alone decides it.
pub(crate) fn narrow_to_enum_cases(
    var_name: &str,
    cases: Vec<ResolvedType>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    seed_synthetic_key_if_needed(var_name, scope, ctx);
    let types = scope.get(var_name);
    let admits = types.is_empty()
        || cases.iter().any(|case| {
            types.iter().any(|rt| {
                rt.type_string
                    .union_members()
                    .iter()
                    .any(|member| member_admits_case(member, &case.type_string, ctx))
            })
        });
    if admits {
        scope.set(var_name, cases);
        write_offset_key_into_shapes(var_name, scope);
    }
}

/// Whether a value of type `member` can be the enum case `case`.
fn member_admits_case(member: &PhpType, case: &PhpType, ctx: &ForwardWalkCtx<'_>) -> bool {
    if case.is_subtype_of(member) {
        return true;
    }
    if member.as_enum_case().is_some() {
        return false;
    }
    let (Some((enum_name, _)), TypeKind::Named(name)) = (case.as_enum_case(), member.kind()) else {
        return false;
    };
    name.eq_ignore_ascii_case("object")
        || crate::class_lookup::is_subclass_of(enum_name, name, ctx.file.class_loader)
}

/// Rule the enum cases `cases` out of `var_name`.
///
/// A case drops out wherever it is held, and the whole enum gives way to
/// the cases it has left, which is what lets an `elseif` chain over an
/// enum's cases know which ones remain. A subject that held nothing else is
/// exhausted.
pub(crate) fn subtract_enum_cases(
    var_name: &str,
    cases: &[ResolvedType],
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    seed_synthetic_key_if_needed(var_name, scope, ctx);
    let mut types = scope.get(var_name).to_vec();
    if types.is_empty() {
        return;
    }
    let mut changed = false;
    for case in cases {
        let (Some(enum_class), Some((_, case_name))) =
            (&case.class_info, case.type_string.as_enum_case())
        else {
            continue;
        };
        let mut kept = Vec::with_capacity(types.len());
        for rt in types {
            match without_enum_case(&rt, enum_class, case_name) {
                Some(remaining) => {
                    changed = true;
                    kept.extend(remaining);
                }
                None => kept.push(rt),
            }
        }
        types = kept;
    }
    if !changed {
        return;
    }
    if types.is_empty() {
        mark_exhausted(var_name, scope);
    } else {
        scope.set(var_name, types);
        write_offset_key_into_shapes(var_name, scope);
    }
}

/// What is left of `rt` once the case `case_name` of `enum_class` is ruled
/// out of it: one entry for each case it can still be, and one for the
/// alternatives that are not the enum at all. `None` when the case was
/// never one of its values.
fn without_enum_case(
    rt: &ResolvedType,
    enum_class: &Arc<ClassInfo>,
    case_name: &str,
) -> Option<Vec<ResolvedType>> {
    let enum_fqn = enum_class.fqn();
    let is_enum = |name: &str| name.eq_ignore_ascii_case(&enum_fqn);
    let mut changed = false;
    let mut cases: Vec<PhpType> = Vec::new();
    let mut others: Vec<PhpType> = Vec::new();
    let mut members: Vec<PhpType> = Vec::new();
    for member in rt.type_string.union_members() {
        match member.kind() {
            TypeKind::Nullable(inner) => members.extend([inner.clone(), PhpType::null()]),
            _ => members.push(member.clone()),
        }
    }
    for member in members {
        if let Some((enum_name, case)) = member.as_enum_case()
            && is_enum(enum_name)
        {
            if case == case_name {
                changed = true;
            } else {
                cases.push(member.clone());
            }
            continue;
        }
        if let TypeKind::Named(name) = member.kind()
            && is_enum(name)
        {
            changed = true;
            cases.extend(
                enum_class
                    .constants
                    .iter()
                    .filter(|c| c.is_enum_case && c.name != case_name)
                    .map(|c| PhpType::enum_case(enum_fqn, c.name)),
            );
            continue;
        }
        others.push(member);
    }
    if !changed {
        return None;
    }
    let mut remaining: Vec<ResolvedType> = cases
        .into_iter()
        .map(|case| ResolvedType::from_both_arc(case, Arc::clone(enum_class)))
        .collect();
    if !others.is_empty() {
        let mut rest = rt.clone();
        rest.replace_type(match others.len() {
            1 => others.swap_remove(0),
            _ => PhpType::union(others),
        });
        remaining.push(rest);
    }
    Some(remaining)
}

/// The enum cases the arms of a `match` compare their subject against.
///
/// A `match` compares with `===`, so an arm whose every condition is a case
/// runs with the subject narrowed to those cases, and reaching an arm (or
/// `default`) means no case an arm above it named was the subject.
pub(crate) struct MatchEnumCaseArms {
    subject: String,
    /// Per arm, the cases its conditions name. `None` for `default`, and
    /// for an arm one of whose conditions is not a case.
    arm_cases: Vec<Option<Vec<ResolvedType>>>,
    /// Per arm, the cases among its conditions, whatever else it compares
    /// against: each is ruled out below the arm.
    named_cases: Vec<Vec<ResolvedType>>,
}

impl MatchEnumCaseArms {
    /// Read the case conditions of `match_expr`'s arms, or `None` when the
    /// subject is not something a scope tracks or no arm names a case.
    pub(crate) fn of(
        match_expr: &Match<'_>,
        scope: &ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) -> Option<Self> {
        let names_constant = match_expr.arms.iter().any(|arm| match arm {
            MatchArm::Expression(arm) => arm.conditions.iter().any(|condition| {
                matches!(
                    unwrap_parens(condition),
                    Expression::Access(Access::ClassConstant(_))
                )
            }),
            MatchArm::Default(_) => false,
        });
        if !names_constant {
            return None;
        }
        let subject = expr_to_subject(match_expr.expression)?;
        let mut arm_cases = Vec::with_capacity(match_expr.arms.len());
        let mut named_cases = Vec::with_capacity(match_expr.arms.len());
        for arm in match_expr.arms.iter() {
            let MatchArm::Expression(arm) = arm else {
                arm_cases.push(None);
                named_cases.push(Vec::new());
                continue;
            };
            let cases: Vec<Option<ResolvedType>> = arm
                .conditions
                .iter()
                .map(|condition| enum_case_comparand(condition, scope, ctx))
                .collect();
            arm_cases.push(cases.iter().cloned().collect::<Option<Vec<_>>>());
            named_cases.push(cases.into_iter().flatten().collect());
        }
        if named_cases.iter().all(Vec::is_empty) {
            return None;
        }
        Some(Self {
            subject,
            arm_cases,
            named_cases,
        })
    }

    /// Narrow `scope` to what the arm at `index` sees of the subject.
    pub(crate) fn narrow_arm(
        &self,
        index: usize,
        scope: &mut ScopeState,
        ctx: &ForwardWalkCtx<'_>,
    ) {
        // `default` runs only once every other arm failed, wherever it is
        // written.
        let is_default = self.arm_cases[index].is_none() && self.named_cases[index].is_empty();
        let failed: Vec<ResolvedType> = self
            .named_cases
            .iter()
            .enumerate()
            .filter(|(other, _)| {
                if is_default {
                    *other != index
                } else {
                    *other < index
                }
            })
            .flat_map(|(_, cases)| cases.iter().cloned())
            .collect();
        if !failed.is_empty() {
            subtract_enum_cases(&self.subject, &failed, scope, ctx);
        }
        if let Some(cases) = &self.arm_cases[index]
            && !scope.unreachable
        {
            narrow_to_enum_cases(&self.subject, cases.clone(), scope, ctx);
        }
    }
}
