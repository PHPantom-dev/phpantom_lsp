//! Binding a method's `@template` parameters from its
//! `@psalm-if-this-is` pattern, matched against the receiver's type.

use std::collections::HashMap;

use crate::atom::{Atom, AtomMap};
use crate::php_type::{PhpType, TypeKind};

/// Infer template parameter substitutions from a `@psalm-if-this-is` pattern
/// by matching it against the receiver's concrete type.
///
/// For example, given:
/// - `pattern`: `ArrayList<TOption|TEither>`
/// - `receiver`: `ArrayList<Either<Exception, int>|Option<int>>`
/// - Method templates: `A`, `B`, `TOption of Option<A>`, `TEither of Either<mixed, B>`
///
/// This matches `TOption → Option<int>`, `TEither → Either<Exception, int>`,
/// then extracts `A = int` from `Option<A>` vs `Option<int>`, and
/// `B = int` from `Either<mixed, B>` vs `Either<Exception, int>`.
pub(super) fn infer_if_this_is_subs(
    pattern: &PhpType,
    receiver: &PhpType,
    template_params: &[Atom],
    template_bounds: &AtomMap<PhpType>,
) -> HashMap<String, PhpType> {
    let mut subs: HashMap<String, PhpType> = HashMap::new();

    // Step 1: Match the top-level structure (e.g. Generic vs Generic)
    // and collect direct template bindings.
    match_type_pattern(
        pattern,
        receiver,
        template_params,
        template_bounds,
        &mut subs,
    );

    // Step 2: For each matched template that has a bound with nested
    // templates, match the bound against the concrete value to extract
    // the nested template parameters.
    let direct_subs = subs.clone();
    for (tpl_name, concrete_type) in &direct_subs {
        let tpl_atom = crate::atom::atom(tpl_name);
        if let Some(bound) = template_bounds.get(&tpl_atom) {
            match_type_pattern(
                bound,
                concrete_type,
                template_params,
                template_bounds,
                &mut subs,
            );
        }
    }

    subs
}

/// Recursively match a type pattern against a concrete type, collecting
/// template parameter bindings into `subs`.
fn match_type_pattern(
    pattern: &PhpType,
    concrete: &PhpType,
    template_params: &[Atom],
    template_bounds: &AtomMap<PhpType>,
    subs: &mut HashMap<String, PhpType>,
) {
    match (pattern.kind(), concrete.kind()) {
        // A named type that is a template parameter — bind it.
        (TypeKind::Named(name), _)
            if template_params.iter().any(|t| t.as_str() == name.as_str()) =>
        {
            subs.entry(name.to_string())
                .or_insert_with(|| concrete.clone());
        }
        // Generic types with matching base names — recurse into args.
        (TypeKind::Generic(p), TypeKind::Generic(c))
            if p.name == c.name && p.args.len() == c.args.len() =>
        {
            for (p_arg, c_arg) in p.args.iter().zip(c.args.iter()) {
                match_type_pattern(p_arg, c_arg, template_params, template_bounds, subs);
            }
        }
        // Union types — match pattern members against concrete members
        // by trying to pair each template pattern member with a concrete
        // member whose base name matches the template's bound.
        (TypeKind::Union(p_members), TypeKind::Union(c_members)) => {
            for p_m in p_members {
                if let TypeKind::Named(name) = p_m.kind() {
                    if template_params.iter().any(|t| t.as_str() == name.as_str()) {
                        // This pattern member is a template param in a union.
                        // Find the concrete union member whose base name
                        // matches this template's bound base name.
                        let tpl_atom = crate::atom::atom(name);
                        if let Some(bound) = template_bounds.get(&tpl_atom) {
                            let bound_base = bound.base_name().unwrap_or_default();
                            for c_m in c_members {
                                let c_base = c_m.base_name().unwrap_or_default();
                                if c_base == bound_base {
                                    subs.entry(name.to_string()).or_insert_with(|| c_m.clone());
                                    break;
                                }
                            }
                        } else {
                            // No bound — take the first concrete member.
                            if let Some(c_m) = c_members.first() {
                                subs.entry(name.to_string()).or_insert_with(|| c_m.clone());
                            }
                        }
                    }
                } else {
                    // Non-template pattern member — recurse.
                    for c_m in c_members {
                        if p_m.base_name() == c_m.base_name() {
                            match_type_pattern(p_m, c_m, template_params, template_bounds, subs);
                            break;
                        }
                    }
                }
            }
        }
        // Nullable patterns.
        (TypeKind::Nullable(p_inner), TypeKind::Nullable(c_inner)) => {
            match_type_pattern(p_inner, c_inner, template_params, template_bounds, subs);
        }
        _ => {}
    }
}
