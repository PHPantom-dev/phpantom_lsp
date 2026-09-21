use super::*;

/// Apply null narrowing for the truthy branch.
///
/// Handles `$x !== null`, `$x != null`, `isset($x)`, `!empty($x)`,
/// `!is_null($x)`, and truthiness checks.
pub(crate) fn apply_null_narrowing_truthy<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Decompose `&&` chains so that `isset($a) && isset($b)` narrows
    // both variables, and `$x !== null && $y !== null` works too.
    let operands = collect_and_chain_operands(condition);
    if operands.len() > 1 {
        for operand in &operands {
            apply_null_narrowing_truthy(operand, scope, ctx);
        }
        return;
    }

    // Check for `$x !== null` or `$x != null` or `null !== $x` etc.
    if let Some(var_name) = extract_non_null_check_var(condition) {
        // For array access keys, narrow the shape on the base variable.
        strip_null_from_subject(&var_name, scope, ctx);
    }
    // Check for `$x !== false` or `false !== $x` — the truthy branch
    // rules out `false` alone, which is what the `T|false` handle idiom
    // (`fopen()`, `finfo_open()`, `strpos()`, …) is written to do.
    if let Some(var_name) = extract_non_false_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_false_from_scope(&var_name, scope);
    }
    // `isset($x)` — truthy branch means $x is not null: strip null.
    // Handles multiple args: `isset($a, $b)` strips null from both.
    for var_name in extract_isset_vars(condition) {
        strip_null_from_subject(&var_name, scope, ctx);
    }
    // `!isset($x)` — truthy branch means $x is null: narrow to null.
    for var_name in extract_not_isset_vars(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // Check for `$x === null` or `$x == null` — narrow to null only.
    if let Some(var_name) = extract_null_equality_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `$x !== ''` / `$x !== []` — refine to the non-empty counterpart, and
    // `$x === ''` to the empty one.
    if let Some((var_name, empty, non_empty)) = extract_empty_value_check(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        if non_empty {
            refine_non_empty_in_scope(&var_name, empty, scope);
        } else {
            refine_empty_in_scope(&var_name, empty, scope);
        }
    }
    // `count($x) > 0` — the counted subject has entries, which is what a
    // `foreach` guarded this way reads to know its body runs.  `count($x)
    // === 0` says the subject is the empty array.
    if let Some((var_name, non_empty, _)) = extract_count_emptiness_check(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        if non_empty {
            refine_non_empty_in_scope(&var_name, EmptyValue::Array, scope);
        } else {
            refine_empty_in_scope(&var_name, EmptyValue::Array, scope);
        }
    }
    // `$x === 0` / `$x !== []` and the rest of the strict comparisons
    // against a written-out value: the equal branch holds that value, the
    // unequal one holds everything else the subject could be.
    apply_literal_identity_narrowing(condition, scope, ctx, true);
    // `$x === Land::Be` — the subject holds whatever the constant holds,
    // so a constant that cannot be null leaves no null in the subject.
    if let Some((var_name, constant)) = extract_class_constant_identity(condition, true) {
        strip_null_by_constant_identity(&var_name, constant, scope, ctx);
    }
    // `$x === $y` — the same reasoning for any comparand whose own type
    // rules out null.
    apply_identity_comparison_null_narrowing(condition, scope, ctx, true);
    // `!empty($x)` — truthy branch means $x is non-empty (truthy):
    // strip null and false from the type.
    if let Some(var_name) = extract_not_empty_var(condition) {
        // `!empty($arr['key'])` says of the key what `isset` does — it is
        // there — so the shape drops its `?` as well as the falsy half of
        // its value type.
        if let Some((base, key)) = split_array_access_key(&var_name) {
            strip_null_from_array_shape_key(base, key, scope);
        }
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_falsy_from_scope(&var_name, scope);
    }
    // Bare truthy check: `if ($x) { ... }` — $x is truthy in the
    // then-body, so strip null and false from its type.
    if let Some(var_name) = expr_to_subject(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_falsy_from_scope(&var_name, scope);
    }
}

/// Apply inverse null narrowing (for guard clause: `if ($x === null) { return; }`).
pub(crate) fn apply_null_narrowing_inverse<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // Decompose `||` chains: `if (A || B) { return; }` only falls
    // through to the rest of the function when every operand is false,
    // so each operand's own inverse narrowing holds on its own — the
    // same De Morgan reasoning `apply_condition_narrowing_inverse` uses
    // for instanceof checks, applied here to null/false checks.
    let or_operands = collect_or_chain_operands(condition);
    if or_operands.len() > 1 {
        for operand in &or_operands {
            apply_null_narrowing_inverse(operand, scope, ctx);
        }
        return;
    }

    // When the condition is `$x === null` (equality check for null),
    // the inverse (else/guard) means $x is NOT null.
    if let Some(var_name) = extract_null_equality_check_var(condition) {
        // For array access keys like `$a["test"]`, narrow the array
        // shape on the base variable directly rather than using a
        // synthetic scope entry.  This ensures the narrowed shape
        // survives scope merges.
        strip_null_from_subject(&var_name, scope, ctx);
    }
    // When the condition is `$x !== null`, the inverse (else/guard)
    // means $x IS null — narrow to null only.
    if let Some(var_name) = extract_non_null_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // When the condition is `!$x` or `empty($x)`, the inverse means
    // $x is truthy — remove every falsy member, not just `null`.  This
    // is the same proof the fall-through of `if (!$x) { return; }`
    // carries, so it strips the same set: an else branch that only lost
    // `null` leaves `false` in a `string|false` union to reappear at the
    // merge, undoing a reassignment the taken branch made to repair it.
    if let Some(var_name) = extract_falsy_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_falsy_from_scope(&var_name, scope);
    }
    // When the condition is `$x === false`, the inverse (else/guard)
    // means $x is NOT false — strip false only, mirroring the null
    // equality case above.
    if let Some(var_name) = extract_false_equality_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_false_from_scope(&var_name, scope);
    }
    // When the condition is `$x !== false`, the inverse (else/guard)
    // means $x IS false — narrow to false only.
    if let Some(var_name) = extract_non_false_check_var(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_false_in_scope(&var_name, scope);
    }
    // The else branch of a strict comparison against a written-out value
    // establishes the opposite of what the body did.
    apply_literal_identity_narrowing(condition, scope, ctx, false);
    // When the condition is `$x !== Land::Be`, the inverse (else/guard)
    // means the subject is that constant, so it holds whatever the
    // constant holds.
    if let Some((var_name, constant)) = extract_class_constant_identity(condition, false) {
        strip_null_by_constant_identity(&var_name, constant, scope, ctx);
    }
    // When the condition is `$x !== $y`, the inverse (else/guard) means
    // the two were identical, so a comparand that cannot be null leaves
    // no null in the subject.
    apply_identity_comparison_null_narrowing(condition, scope, ctx, false);
    // When the condition is `$x === ''` / `$x === []`, the inverse
    // (else/guard) means $x is non-empty; when it is `$x !== ''`, the
    // inverse means $x is exactly the empty value.
    if let Some((var_name, empty, non_empty)) = extract_empty_value_check(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        if non_empty {
            refine_empty_in_scope(&var_name, empty, scope);
        } else {
            refine_non_empty_in_scope(&var_name, empty, scope);
        }
    }
    // When the condition is `count($x) === 0`, the inverse (else, or the
    // fall-through of a guard that threw) means the subject has entries;
    // when it is `count($x) > 0`, the inverse means it has none.  A bound
    // further out (`count($x) > 1`) proves neither on the way past.
    if let Some((var_name, non_empty, complement_exact)) = extract_count_emptiness_check(condition)
        && complement_exact
    {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        if non_empty {
            refine_empty_in_scope(&var_name, EmptyValue::Array, scope);
        } else {
            refine_non_empty_in_scope(&var_name, EmptyValue::Array, scope);
        }
    }
    // When the condition is a bare `$x` (truthy check), the inverse means
    // $x is falsy: every member that could not have been drops out, which
    // is what leaves `while ($a) { … }` with a `null` below it.  A `bool`
    // keeps its `false` half rather than staying whole, and that is what
    // lets a branch the flag guards be recognised again where the flag is
    // re-tested.
    //
    // A path (`$row->id`) or a call (`$id->isClass()`) is tested exactly
    // as a variable is, and the truthy side already narrows both under
    // their own key.  The skipped path has to say `false` about the same
    // key or the two sides of the `if` describe values that could be the
    // same one, and the join has nothing to key the branch's writes
    // against.
    if let Some(var_name) = expr_to_subject(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_falsy_in_scope(&var_name, scope);
    }
    // `isset($x)` — inverse (else) means $x was null: narrow to null.
    for var_name in extract_isset_vars(condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        narrow_to_null_in_scope(&var_name, scope);
    }
    // `!isset($x)` — inverse (guard after `!isset` return) means $x
    // is not null: strip null.
    for var_name in extract_not_isset_vars(condition) {
        strip_null_from_subject(&var_name, scope, ctx);
    }
}

/// Narrow the receivers of every nullsafe chain the condition proves is
/// not `null`.
///
/// `if ($image?->file_id !== null)` can only be entered when `$image`
/// itself is not null: had it been, the chain would have short-circuited
/// to `null` and the comparison would have failed.  A truthy test on the
/// chain and an identity check against a non-null value carry the same
/// proof, and a chain of several `?->` links proves it for each receiver
/// along the way.
///
/// `truthy` is the polarity the caller establishes: `true` for an `if`
/// body, `false` for an else branch or the fall-through of a guard clause
/// that leaves the scope.
pub(crate) fn apply_nullsafe_receiver_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
    truthy: bool,
) {
    let mut proven: Vec<&Expression<'_>> = Vec::new();
    collect_proven_non_null_exprs(condition, truthy, &mut proven);

    let mut keys: Vec<String> = Vec::new();
    for expr in proven {
        for key in nullsafe_receiver_keys(expr) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }

    // A chain compared identical to a value that cannot be null held a
    // non-null value itself, which is the same proof about its receivers.
    let mut compared: Vec<(&Expression<'_>, &Expression<'_>)> = Vec::new();
    collect_identity_comparisons(condition, truthy, &mut compared);
    for (chain, comparand) in compared {
        let chain_keys: Vec<String> = nullsafe_receiver_keys(chain)
            .into_iter()
            .filter(|key| !keys.contains(key))
            .collect();
        // Resolving the comparand is the expensive half, so it happens
        // only once the cheap half has found receivers still to prove.
        if chain_keys.is_empty() || expr_accepts_null(comparand, scope, ctx) {
            continue;
        }
        keys.extend(chain_keys);
    }

    for key in keys {
        seed_synthetic_key_if_needed(&key, scope, ctx);
        strip_null_from_scope(&key, scope);
    }
}

/// The scope keys of every receiver a nullsafe chain short-circuits on,
/// outermost first.  Empty when `expr` holds no `?->` link.
///
/// The walk peels plain `->` links too, because a `?->` short-circuits the
/// rest of the chain written after it: `$a?->b()->c()` is `null` whenever
/// `$a` is, so a proof about the whole chain is a proof about `$a`.  Only
/// the `?->` receivers are recorded — a plain link's receiver is one the
/// chain would have thrown on, not short-circuited.
fn nullsafe_receiver_keys(expr: &Expression<'_>) -> Vec<String> {
    let mut keys = Vec::new();
    let mut node = expr;
    while let Some((receiver, nullsafe)) = chain_receiver(node) {
        if nullsafe && let Some(key) = narrowing::expr_to_subject_key(receiver) {
            keys.push(key);
        }
        node = receiver;
    }
    keys
}

/// Record what the expression assigned to `lhs_name` proves about the
/// other keys it read, for a guard that arrives later and only names the
/// result.
///
/// Two shapes carry such a proof. A `?->` chain's null stands for its
/// receivers': `$period = $agreement?->latestPeriod();` followed by
/// `if (!$period instanceof Period) { return; }` proves `$agreement` is
/// not null past the guard — the chain would have short-circuited to
/// `null` otherwise. And a plain copy of a member path holds the very
/// same value, so `$cacheKey = $this->cacheKey;` makes a later
/// `$cacheKey !== null` a proof about `$this->cacheKey` too. Either way
/// the guard's condition never names the other key, so the link has to be
/// recorded where it is written.
///
/// Nothing is recorded when the assigned value is not nullable: without a
/// `null` to rule out, "not null" is not evidence that any guard ran.
pub(crate) fn record_nullsafe_origin<'b>(
    lhs_name: &str,
    rhs: &'b Expression<'b>,
    scope: &mut ScopeState,
) {
    if !scope_value_is_nullable(lhs_name, scope) {
        return;
    }
    let mut implied = nullsafe_receiver_keys(rhs);
    // A copy reads the value; a call only reads the receiver, and what it
    // returns is its own value, not the receiver's.
    if let Some(key) = narrowing::expr_to_subject_key(rhs)
        && narrowing::is_member_path_key(&key)
        && !narrowing::is_call_key(&key)
    {
        implied.push(key);
    }
    // A path rooted at the variable being written names a different value
    // after the write than it did before it: `$a = $a->a;` leaves `$a->a`
    // meaning the *new* `$a`'s property, which the assignment proves
    // nothing about. Recording it would undo the invalidation the write
    // just performed.
    let implied: Vec<Atom> = implied
        .iter()
        .filter(|key| key.as_str() != lhs_name && !narrowing::key_reads_variable(key, lhs_name))
        .map(|key| atom(key))
        .collect();
    scope.record_non_null_implication(lhs_name, implied);
}

/// The receiver one link of a member chain is applied to, paired with
/// whether that link is the nullsafe `?->`.
fn chain_receiver<'b>(expr: &'b Expression<'b>) -> Option<(&'b Expression<'b>, bool)> {
    match expr {
        Expression::Parenthesized(inner) => chain_receiver(inner.expression),
        Expression::Access(Access::NullSafeProperty(pa)) => Some((pa.object, true)),
        Expression::Call(Call::NullSafeMethod(mc)) => Some((mc.object, true)),
        Expression::Access(Access::Property(pa)) => Some((pa.object, false)),
        Expression::Call(Call::Method(mc)) => Some((mc.object, false)),
        _ => None,
    }
}

pub(crate) fn apply_guard_clause_null_narrowing<'b>(
    if_stmt: &'b If<'b>,
    scope: &mut ScopeState,
    ctx: &ForwardWalkCtx<'_>,
) {
    // When `if ($x === null) { return; }`, strip null from $x after.
    // When `if (!$x) { return; }`, strip null from $x after.
    if let Some(var_name) = extract_null_equality_check_var(if_stmt.condition) {
        strip_null_from_subject_shape(&var_name, scope, ctx);
    }
    if let Some(var_name) = extract_falsy_check_var(if_stmt.condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_falsy_from_scope(&var_name, scope);
    }
    // When `if ($x === false) { throw; }`, strip only `false` from $x
    // after — the common "resource-like handle" idiom (`finfo_open()`,
    // `pg_connect()`, …) that returns `T|false`.
    if let Some(var_name) = extract_false_equality_check_var(if_stmt.condition) {
        seed_synthetic_key_if_needed(&var_name, scope, ctx);
        strip_false_from_scope(&var_name, scope);
    }
    // `if (!isset($x)) { return; }` — after the guard, $x is not null.
    for var_name in extract_not_isset_vars(if_stmt.condition) {
        strip_null_from_subject_shape(&var_name, scope, ctx);
    }
    // `if ($x !== null)` with return doesn't narrow after — the
    // remaining code is the null path.  This is handled by the
    // inverse narrowing in the guard clause logic.
}
