//! Docblock enrichment: propagating type information from ancestors.
//!
//! When a child class lacks docblock-provided type information (return types,
//! parameter types, descriptions), this module propagates richer type information
//! from ancestors through inheritance chains.

use std::sync::Arc;

use crate::php_type::PhpType;
use crate::types::{MethodInfo, ParameterInfo, PropertyInfo};

/// Whether a child's effective type equals its native type, meaning no
/// docblock override was applied.
///
/// Returns `true` when the child wrote no `@return` / `@var` / `@param`
/// tag (so the effective type is just the native hint).  Returns `false`
/// when the child provided its own docblock type — in that case the
/// child's type is an intentional override and should not be replaced.
fn lacks_docblock_override(effective: &Option<PhpType>, native: &Option<PhpType>) -> bool {
    match (effective, native) {
        // No effective type at all — nothing to override.
        (None, _) => true,
        // Effective type present but no native type — the child wrote
        // a docblock-only type (e.g. `@return list<Pen>` with no native
        // hint).  That is an intentional override.
        (Some(_), None) => false,
        // Both present — if they are equivalent, the child didn't write
        // a docblock (the effective type is just the native hint echoed).
        (Some(eff), Some(nat)) => eff.equivalent(nat),
    }
}

/// Whether an ancestor's type is richer than the child's native type.
///
/// Returns `true` when the ancestor has an effective type that differs
/// from its own native type (meaning the ancestor wrote a docblock).
fn ancestor_has_richer_type(effective: &Option<PhpType>, native: &Option<PhpType>) -> bool {
    match (effective, native) {
        // Ancestor has an effective type but no native type — it came
        // from a docblock (e.g. interface method with `@return list<Pen>`
        // and no native hint).
        (Some(_), None) => true,
        // Both present — richer if they differ (docblock overrides native).
        (Some(eff), Some(nat)) => !eff.equivalent(nat),
        // No effective type — nothing richer to offer.
        _ => false,
    }
}

/// The return type `existing` should take from `ancestor`, or `None` when
/// it keeps its own.
///
/// The ancestor's docblock applies when (a) the child has no return type at
/// all, or (b) the child's effective type equals its native type (no
/// docblock override of its own) and the ancestor has a richer docblock
/// type. The child's *native* hint still has the last word: an override
/// declaring `: array` cannot return the `string` half of an interface's
/// `@return array|string`, so the inherited union is restricted to what the
/// override's own declaration allows.
fn inherited_return_type(existing: &MethodInfo, ancestor: &MethodInfo) -> Option<PhpType> {
    if !(existing.return_type.is_none() && ancestor.return_type.is_some()
        || lacks_docblock_override(&existing.return_type, &existing.native_return_type)
            && ancestor_has_richer_type(&ancestor.return_type, &ancestor.native_return_type))
    {
        return None;
    }

    let inherited = ancestor.return_type.as_ref()?;
    Some(match existing.native_return_type {
        Some(ref native) => inherited.without_alternatives_the_native_type_forbids(native),
        None => inherited.clone(),
    })
}

/// Copy-on-write wrapper around [`enrich_method_from_ancestor`].
///
/// Most class/ancestor method overlaps have nothing to propagate (the
/// child already carries its own docblock types, or the ancestor has
/// nothing richer to offer).  Checking first means the shared
/// `Arc<MethodInfo>` is only deep-cloned when the enrichment actually
/// changes a field, instead of once per overlap per class.
pub(crate) fn enrich_method_arc_from_ancestor(
    existing: &mut Arc<MethodInfo>,
    ancestor: &MethodInfo,
) {
    if method_enrichment_would_change(existing, ancestor) {
        enrich_method_from_ancestor(Arc::make_mut(existing), ancestor);
    }
}

/// Whether [`enrich_method_from_ancestor`] would change any field of
/// `existing`.
///
/// Mirrors the apply function's conditions exactly (keep the two in
/// sync), with one refinement: a copy that would overwrite a field with
/// an equal value counts as "no change".  A `false` here guarantees the
/// enrichment is semantically a no-op, so the caller can skip the
/// copy-on-write clone.
fn method_enrichment_would_change(existing: &MethodInfo, ancestor: &MethodInfo) -> bool {
    // Return type.
    if let Some(inherited) = inherited_return_type(existing, ancestor)
        && existing.return_type.as_ref() != Some(&inherited)
    {
        return true;
    }

    // Template parameters (copies bounds and bindings alongside).
    if existing.template_params.is_empty() && !ancestor.template_params.is_empty() {
        return true;
    }

    // Conditional return type.
    if existing.conditional_return.is_none() && ancestor.conditional_return.is_some() {
        return true;
    }

    // Type assertions.
    if existing.type_assertions.is_empty() && !ancestor.type_assertions.is_empty() {
        return true;
    }

    // Parameters (constructors match by name, everything else by position).
    let params_would_change = if existing.name == "__construct" {
        constructor_parameters_would_change(&existing.parameters, &ancestor.parameters)
    } else {
        positional_parameters_would_change(&existing.parameters, &ancestor.parameters)
    };
    if params_would_change {
        return true;
    }

    // Deprecation (an override with no docblock of its own inherits the
    // ancestor's `@deprecated` tag, matching PHPStan's phpDoc-inheritance
    // resolver).
    if existing.deprecation_message.is_none() && ancestor.deprecation_message.is_some() {
        return true;
    }

    // Descriptions.
    (existing.description.is_none() && ancestor.description.is_some())
        || (existing.return_description.is_none() && ancestor.return_description.is_some())
}

/// Whether [`enrich_constructor_parameters_by_name`] would change any
/// parameter.
fn constructor_parameters_would_change(
    existing_params: &[ParameterInfo],
    ancestor_params: &[ParameterInfo],
) -> bool {
    existing_params.iter().any(|ep| {
        ancestor_params
            .iter()
            .find(|ap| ap.name == ep.name)
            .is_some_and(|ap| parameter_enrichment_would_change(ep, ap))
    })
}

/// Whether [`enrich_parameters_from_ancestor`] would change any
/// parameter.
fn positional_parameters_would_change(
    existing_params: &[ParameterInfo],
    ancestor_params: &[ParameterInfo],
) -> bool {
    existing_params
        .iter()
        .zip(ancestor_params)
        .any(|(ep, ap)| parameter_enrichment_would_change(ep, ap))
}

/// Whether [`enrich_single_parameter`] would change `existing_param`.
///
/// Mirrors the apply function's conditions (keep the two in sync).
/// Whether the child's own native parameter hint supersedes the
/// ancestor's docblock type.
///
/// PHP requires an override to restate every parameter's type hint, so a
/// child that repeats the ancestor's hint verbatim has declared nothing
/// new: the ancestor's `@param` docblock (with template substitutions
/// from `@extends`/`@implements` already applied) still describes the
/// value that reaches the body.  That is what narrows a
/// `@implements Rule<CallLike>` implementation's `Node $node` parameter
/// to `CallLike`, matching how PHPStan inherits PHPDoc.
///
/// A child that names a *different* specific type has made a deliberate
/// declaration — parameters are contravariant, so it may legitimately be
/// wider than the ancestor's — and keeps it.
fn child_native_hint_overrides(
    existing_param: &ParameterInfo,
    ancestor_param: &ParameterInfo,
) -> bool {
    existing_param.native_type_hint != ancestor_param.native_type_hint
        && existing_param.native_type_hint.as_ref().is_some_and(|nt| {
            !nt.is_object() && !nt.is_mixed() && !nt.is_array_like() && !nt.is_iterable()
        })
}

fn parameter_enrichment_would_change(
    existing_param: &ParameterInfo,
    ancestor_param: &ParameterInfo,
) -> bool {
    if !child_native_hint_overrides(existing_param, ancestor_param)
        && lacks_docblock_override(&existing_param.type_hint, &existing_param.native_type_hint)
        && ancestor_has_richer_type(&ancestor_param.type_hint, &ancestor_param.native_type_hint)
        && existing_param.type_hint != ancestor_param.type_hint
    {
        return true;
    }
    existing_param.description.is_none() && ancestor_param.description.is_some()
}

/// Enrich a child method with docblock information from an ancestor method.
///
/// Propagates return types, parameter types, descriptions, template
/// parameters, conditional return types, and type assertions from the
/// ancestor when the child lacks its own docblock overrides.
///
/// **Return type rule:** If the child's `return_type` equals its
/// `native_return_type` (no docblock), and the ancestor's `return_type`
/// differs from its `native_return_type` (has docblock), copy the
/// ancestor's `return_type` to the child.  If the child has no
/// `return_type` at all, always inherit the ancestor's.
///
/// **Parameter rule:** Match by position (not by name, since the child
/// may rename parameters).  Same effective-vs-native comparison as
/// return types.
///
/// **Description rule:** Inherit `description` and `return_description`
/// when the child has `None`.
pub(crate) fn enrich_method_from_ancestor(existing: &mut MethodInfo, ancestor: &MethodInfo) {
    // ── Return type ─────────────────────────────────────────────
    if let Some(inherited) = inherited_return_type(existing, ancestor) {
        existing.return_type = Some(inherited);
    }

    // ── Template parameters ─────────────────────────────────────
    if existing.template_params.is_empty() && !ancestor.template_params.is_empty() {
        existing.template_params = ancestor.template_params.clone();
        existing.template_param_bounds = ancestor.template_param_bounds.clone();
        existing.template_bindings = ancestor.template_bindings.clone();
        // Template return types like `T` only make sense when the
        // template params are present — inherit the return type too
        // if we haven't already set it.
        if existing.return_type.is_none() {
            existing.return_type = ancestor.return_type.clone();
        }
    }

    // ── Conditional return type ─────────────────────────────────
    if existing.conditional_return.is_none() && ancestor.conditional_return.is_some() {
        existing.conditional_return = ancestor.conditional_return.clone();
    }

    // ── Type assertions ─────────────────────────────────────────
    if existing.type_assertions.is_empty() && !ancestor.type_assertions.is_empty() {
        existing.type_assertions = ancestor.type_assertions.clone();
    }

    // ── Parameters ──────────────────────────────────────────────
    // For constructors, use **name-based** matching instead of
    // positional.  PHP constructors don't follow Liskov substitution
    // — a child constructor can have a completely different signature
    // (different parameter count, order, types).  Positional
    // enrichment would incorrectly map ancestor param types onto
    // unrelated child params (e.g. Exception's `$code` type `int`
    // onto a child's `$message` param at position 1).
    //
    // This follows PHPStan's `PhpDocInheritanceResolver`: for
    // `__construct` the positional parameter name list falls back to
    // the child's own names, so only same-named parameters inherit.
    // The would-change pre-check keeps the shared parameter list intact
    // (no copy-on-write) when there is nothing to propagate.
    if existing.name == "__construct" {
        if constructor_parameters_would_change(&existing.parameters, &ancestor.parameters) {
            enrich_constructor_parameters_by_name(
                existing.parameters.make_mut(),
                &ancestor.parameters,
            );
        }
    } else if positional_parameters_would_change(&existing.parameters, &ancestor.parameters) {
        enrich_parameters_from_ancestor(existing.parameters.make_mut(), &ancestor.parameters);
    }

    // ── Deprecation ─────────────────────────────────────────────
    if existing.deprecation_message.is_none() && ancestor.deprecation_message.is_some() {
        existing.deprecation_message = ancestor.deprecation_message.clone();
        existing.deprecated_replacement = ancestor.deprecated_replacement.clone();
    }

    // ── Descriptions ────────────────────────────────────────────
    if existing.description.is_none() && ancestor.description.is_some() {
        existing.description = ancestor.description.clone();
    }
    if existing.return_description.is_none() && ancestor.return_description.is_some() {
        existing.return_description = ancestor.return_description.clone();
    }
}

/// Enrich child parameters from ancestor parameters, matched by position.
///
/// When a child parameter's `type_hint` equals its `native_type_hint`
/// (no docblock override) and the ancestor parameter has a richer type,
/// copy the ancestor's `type_hint`.  Also inherit `description` when
/// the child parameter has none.
pub(crate) fn enrich_parameters_from_ancestor(
    existing_params: &mut [ParameterInfo],
    ancestor_params: &[ParameterInfo],
) {
    for (existing_param, ancestor_param) in existing_params.iter_mut().zip(ancestor_params) {
        enrich_single_parameter(existing_param, ancestor_param);
    }
}

/// Enrich constructor parameters from ancestor parameters, matched by name.
///
/// Unlike regular methods (which follow Liskov substitution and can
/// safely use positional matching), constructors can have completely
/// different signatures.  Only parameters with the **same name** in
/// both the child and ancestor are enriched.
pub(crate) fn enrich_constructor_parameters_by_name(
    existing_params: &mut [ParameterInfo],
    ancestor_params: &[ParameterInfo],
) {
    for existing_param in existing_params.iter_mut() {
        if let Some(ancestor_param) = ancestor_params
            .iter()
            .find(|ap| ap.name == existing_param.name)
        {
            enrich_single_parameter(existing_param, ancestor_param);
        }
    }
}

/// Enrich a single child parameter from an ancestor parameter.
///
/// Copies the ancestor's `type_hint` when the child lacks a docblock
/// override, the ancestor has a richer type, **and** the child's
/// native type is not a specific concrete type.
///
/// PHP allows contravariant parameter types: a concrete class may
/// declare `?int` where the interface says `int`, or Carbon's
/// `setTimezone(DateTimeZone|string|int)` may widen DateTime's
/// `setTimezone(DateTimeZone)`.  In those cases the child's native
/// type is an intentional widening and must not be narrowed.
///
/// However, when the child's native type is a placeholder like
/// `object` or `mixed` (common in `@implements`/`@extends` generic
/// patterns where the interface declares `object $entity` and the
/// `@implements` tag substitutes the template to a concrete type),
/// the ancestor's enriched type should flow through.
pub(crate) fn enrich_single_parameter(
    existing_param: &mut ParameterInfo,
    ancestor_param: &ParameterInfo,
) {
    // Type hint enrichment — the child must lack a docblock override
    // AND the ancestor must have a richer type (docblock that goes
    // beyond its native hint), AND the child must not have declared a
    // narrower/wider native hint of its own.
    if !child_native_hint_overrides(existing_param, ancestor_param)
        && lacks_docblock_override(&existing_param.type_hint, &existing_param.native_type_hint)
        && ancestor_has_richer_type(&ancestor_param.type_hint, &ancestor_param.native_type_hint)
    {
        existing_param.type_hint = ancestor_param.type_hint.clone();
    }
    // Description enrichment
    if existing_param.description.is_none() && ancestor_param.description.is_some() {
        existing_param.description = ancestor_param.description.clone();
    }
}

/// Copy-on-write wrapper around [`enrich_property_from_ancestor`].
///
/// The property analogue of [`enrich_method_arc_from_ancestor`]: the
/// shared `Arc<PropertyInfo>` is only deep-cloned when the enrichment
/// would actually change a field, which keeps inherited properties
/// sharing their allocation in the common no-op case.
pub(crate) fn enrich_property_arc_from_ancestor(
    existing: &mut Arc<PropertyInfo>,
    ancestor: &PropertyInfo,
) {
    if property_enrichment_would_change(existing, ancestor) {
        enrich_property_from_ancestor(Arc::make_mut(existing), ancestor);
    }
}

/// Whether [`enrich_property_from_ancestor`] would change any field of
/// `existing`.  Mirrors the apply function's conditions exactly (keep the
/// two in sync); an overwrite with an equal value counts as "no change".
fn property_enrichment_would_change(existing: &PropertyInfo, ancestor: &PropertyInfo) -> bool {
    // Type hint.
    if (existing.type_hint.is_none() && ancestor.type_hint.is_some()
        || lacks_docblock_override(&existing.type_hint, &existing.native_type_hint)
            && ancestor_has_richer_type(&ancestor.type_hint, &ancestor.native_type_hint))
        && existing.type_hint != ancestor.type_hint
    {
        return true;
    }

    // Description.
    existing.description.is_none() && ancestor.description.is_some()
}

/// Enrich a child property with docblock information from an ancestor
/// property.
///
/// Propagates type hints and descriptions from the ancestor when the
/// child lacks its own docblock overrides.  The same
/// effective-vs-native comparison is used as for method return types.
pub(crate) fn enrich_property_from_ancestor(existing: &mut PropertyInfo, ancestor: &PropertyInfo) {
    // ── Type hint ───────────────────────────────────────────────
    // Same logic as method return types: propagate when the child
    // has no type or has only the native hint without a docblock
    // override, and the ancestor provides a richer type.
    if existing.type_hint.is_none() && ancestor.type_hint.is_some()
        || lacks_docblock_override(&existing.type_hint, &existing.native_type_hint)
            && ancestor_has_richer_type(&ancestor.type_hint, &ancestor.native_type_hint)
    {
        existing.type_hint = ancestor.type_hint.clone();
    }

    // ── Description ─────────────────────────────────────────────
    if existing.description.is_none() && ancestor.description.is_some() {
        existing.description = ancestor.description.clone();
    }
}
