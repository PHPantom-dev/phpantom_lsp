use super::*;

// ─── Boolean variables that stand for a check ───────────────────────────────

/// Record the checks a boolean assignment carries.
///
/// `$isHtml = $raw instanceof HtmlString;` proves nothing on its own,
/// but `$isHtml` now stands for the check: wherever it is tested, `$raw`
/// narrows the same way the original expression would narrow it.
///
/// Only the `&&` conjuncts that are themselves `instanceof`-style checks
/// are recorded; anything else in the expression (`$flag`, a comparison,
/// a call) contributes no assertion and is skipped, which still leaves
/// the recorded conjuncts sound for a truthy test.
///
/// A conjunct may itself be an `||` chain over one subject
/// (`$shouldCheck = $n instanceof Function_ || $n instanceof ClassMethod`),
/// which proves the subject is one of the listed classes exactly as the
/// same chain written straight into an `if` does.
pub(crate) fn record_assertion_variable<'b>(
    lhs_name: &str,
    rhs: &'b Expression<'b>,
    scope: &mut ScopeState,
) {
    let mut checks: Vec<VarAssertion> = Vec::new();
    for operand in collect_and_chain_operands(rhs) {
        let mut subjects = collect_condition_var_names(operand);
        subjects.extend(collect_condition_property_keys(operand));
        for subject in subjects {
            // `$x = $x instanceof Foo` overwrites its own subject, so the
            // recorded check would describe a value that no longer exists.
            if subject == lhs_name {
                continue;
            }
            if let Some(mut classes) = or_chain_alternatives(operand, &subject) {
                let class_type = classes.remove(0);
                checks.push(VarAssertion {
                    subject: atom(&subject),
                    class_type,
                    alternatives: classes,
                    negated: false,
                    exact: false,
                    allow_string: false,
                });
                break;
            }
            if let Some(extraction) =
                narrowing::try_extract_instanceof_with_negation(operand, &subject)
            {
                checks.push(VarAssertion {
                    subject: atom(&subject),
                    class_type: extraction.class_type,
                    alternatives: Vec::new(),
                    negated: extraction.negated,
                    exact: extraction.exact,
                    allow_string: extraction.allow_string,
                });
                break;
            }
        }
    }
    if !checks.is_empty() {
        scope.assertions.insert(atom(lhs_name), checks);
    }
}

/// The classes an `||` chain over `subject` proves it is one of.
///
/// Every leg has to be a positive check on the same subject: a leg about
/// some other value, or a negated one, leaves the disjunction proving
/// nothing about `subject`, and collecting only the legs that do match
/// would claim a narrowing the chain never made.
fn or_chain_alternatives(expr: &Expression<'_>, subject: &str) -> Option<Vec<PhpType>> {
    fn walk(expr: &Expression<'_>, subject: &str, out: &mut Vec<PhpType>) -> bool {
        match expr {
            Expression::Parenthesized(inner) => walk(inner.expression, subject, out),
            Expression::Binary(bin)
                if matches!(
                    bin.operator,
                    BinaryOperator::Or(_) | BinaryOperator::LowOr(_)
                ) =>
            {
                walk(bin.lhs, subject, out) && walk(bin.rhs, subject, out)
            }
            _ => match narrowing::try_extract_instanceof_with_negation(expr, subject) {
                Some(extraction) if !extraction.negated => {
                    if !out.contains(&extraction.class_type) {
                        out.push(extraction.class_type);
                    }
                    true
                }
                _ => false,
            },
        }
    }

    let is_or_chain = matches!(
        narrowing::fold_negation_pairs(expr),
        Expression::Binary(bin) if matches!(bin.operator, BinaryOperator::Or(_) | BinaryOperator::LowOr(_))
    );
    if !is_or_chain {
        return None;
    }
    let mut classes = Vec::new();
    (walk(expr, subject, &mut classes) && classes.len() > 1).then_some(classes)
}

/// Every class an alias check allows, its own first.
pub(super) fn alias_classes(alias: &AliasExtraction) -> Vec<PhpType> {
    let mut classes = Vec::with_capacity(alias.alternatives.len() + 1);
    classes.push(alias.extraction.class_type.clone());
    classes.extend(alias.alternatives.iter().cloned());
    classes
}

/// One check a boolean stands for, expanded for the condition testing it.
pub(in crate::type_engine) struct AliasExtraction {
    /// Scope key the check narrows.
    pub subject: String,
    /// The check itself, with the operand's own negation folded in.
    pub extraction: narrowing::InstanceofExtraction,
    /// The further classes an `||` chain allows.  Empty for a check that
    /// names a single class.
    pub alternatives: Vec<PhpType>,
}

/// Expand a bare boolean operand into the checks it stands for.
///
/// `$isHtml` and `!$isHtml` both resolve through the recorded check,
/// with the operand's own negation folded into the result, so the
/// callers below treat them exactly like the original `instanceof`
/// expression.
///
/// A boolean built from several conjuncts only proves its parts when it
/// is true: `!$ok` says one of them failed without saying which, so a
/// negated operand expands only a single-check boolean.
pub(in crate::type_engine) fn assertion_alias_extractions(
    expr: &Expression<'_>,
    scope: &ScopeState,
) -> Vec<AliasExtraction> {
    if scope.assertions.is_empty() {
        return Vec::new();
    }

    let mut negated = false;
    let mut inner = expr;
    loop {
        match inner {
            Expression::Parenthesized(p) => inner = p.expression,
            Expression::UnaryPrefix(prefix) if prefix.operator.is_not() => {
                negated = !negated;
                inner = prefix.operand;
            }
            _ => break,
        }
    }

    let Expression::Variable(Variable::Direct(dv)) = inner else {
        return Vec::new();
    };
    let Some(checks) = scope.assertions.get(&atom(bytes_to_str(dv.name))) else {
        return Vec::new();
    };
    if negated && checks.len() > 1 {
        return Vec::new();
    }

    checks
        .iter()
        .map(|c| AliasExtraction {
            subject: c.subject.to_string(),
            extraction: narrowing::InstanceofExtraction {
                class_type: c.class_type.clone(),
                negated: c.negated != negated,
                exact: c.exact,
                allow_string: c.allow_string,
            },
            alternatives: c.alternatives.clone(),
        })
        .collect()
}
