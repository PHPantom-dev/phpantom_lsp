use super::*;

/// Apply `property_exists($var, 'name')` / `method_exists($var, 'name')`
/// narrowing to the scope.
///
/// In the branch where the guard holds, each class in the variable's
/// resolved union gains a virtual member of the guarded name (unknown
/// type), mirroring PHPStan's `object&hasProperty('name')` intersection.
/// Member access, completion, and hover inside the branch then treat the
/// member as present instead of reporting it unknown.
///
/// `inverted` is `false` for the truthy branch (a bare guard proves the
/// member exists) and `true` for the inverse path (else branch / after an
/// exiting guard clause), where the *negated* form proves it.
pub(crate) fn apply_member_exists_narrowing<'b>(
    condition: &'b Expression<'b>,
    scope: &mut ScopeState,
    inverted: bool,
) {
    for operand in collect_and_chain_operands(condition) {
        let var_names = scope_keys_named_by(operand, scope);
        for var_name in &var_names {
            let Some((member, is_method, negated)) =
                narrowing::try_extract_member_exists_guard(operand, var_name)
            else {
                continue;
            };
            // Only the direction where the guard is known TRUE adds
            // information — "the member does not exist" removes nothing
            // we model.
            if negated != inverted {
                continue;
            }

            let mut results = scope.get(var_name).to_vec();
            let mut changed = false;
            for rt in &mut results {
                let Some(class_info) = &rt.class_info else {
                    continue;
                };
                // Skip when the member is already declared on the class
                // itself — nothing to add, and injecting an untyped
                // virtual member would shadow the declared type.  Only
                // own members are checked (resolving ancestors here
                // would be expensive); guarding a *statically declared
                // inherited* member with `property_exists` is rare, and
                // the cost is an unknown member type inside the branch,
                // never a false diagnostic.
                let already_present = if is_method {
                    class_info.get_method_ci(&member).is_some()
                } else {
                    class_info
                        .properties
                        .iter()
                        .any(|p| p.name.as_str() == member)
                };
                if already_present {
                    continue;
                }
                let mut narrowed = (**class_info).clone();
                if is_method {
                    narrowed
                        .methods
                        .push(Arc::new(MethodInfo::virtual_method(&member, None)));
                } else {
                    narrowed
                        .properties
                        .push(Arc::new(PropertyInfo::virtual_property(&member, None)));
                }
                rt.class_info = Some(Arc::new(narrowed));
                changed = true;
            }
            if changed {
                scope.set(var_name, results);
            }
        }
    }
}
