//! Deciding whether a docblock is out of step with its function: the
//! signature against the `@param` / `@return` tags, and the body's inferred
//! return type and uncaught exceptions against what the docblock says.

use std::collections::HashMap;
use std::sync::Arc;

use crate::Backend;
use crate::class_lookup::{Fqn, ResolvedClassLoader, SourceClassLoader};
use crate::code_actions::phpstan::fix_return_type::enrichment_return_type;
use crate::completion::phpdoc::generation::{enrichment_plain, enrichment_plain_typed};
use crate::completion::source::throws_analysis::{self, ThrowsContext};
use crate::docblock::is_compatible_refinement_typed;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionLoader};

use super::{BodyFacts, DocParam, FunctionWithDocblock};

/// Resolve a type name written in the file to its FQN: through the
/// use-map, else the namespace, then the declared spelling of the class
/// that names, when one loads.
fn resolve_type_name_to_fqn(
    name: &str,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
    class_loader: &ResolvedClassLoader<'_>,
) -> String {
    let resolved = crate::names::resolve_to_fqn(name, use_map, file_namespace);
    class_loader(Fqn::new(&resolved))
        .map(|cls| cls.fqn().to_string())
        .unwrap_or(resolved)
}

/// Run the body analyses (`@return` enrichment and uncaught throws) for
/// the function under the cursor.
#[allow(clippy::too_many_arguments)]
pub(super) fn analyse_body(
    info: &FunctionWithDocblock,
    content: &str,
    local_classes: &[Arc<ClassInfo>],
    class_loader: &SourceClassLoader<'_>,
    resolved_class_loader: &ResolvedClassLoader<'_>,
    backend: Option<&Backend>,
    function_loader: FunctionLoader<'_>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> BodyFacts {
    let enriched_return = match &info.sig_return {
        Some(sig_ret)
            if !sig_ret.is_void()
                && !info
                    .doc_return
                    .as_ref()
                    .is_some_and(|dr| dr.type_parsed.has_type_structure()) =>
        {
            enrichment_return_type(
                crate::type_engine::resolver::FileCtx::new(
                    content,
                    local_classes,
                    resolved_class_loader,
                    class_loader,
                )
                .with_imports(Some(use_map))
                .with_backend(backend),
                info.decl_offset,
                function_loader,
            )
            .filter(|enriched| {
                !enriched.is_void()
                    && !enriched.is_mixed()
                    && !enriched.equivalent(sig_ret)
                    && info
                        .doc_return
                        .as_ref()
                        .is_none_or(|dr| !dr.type_parsed.equivalent(enriched))
            })
        }
        _ => None,
    };

    let uncaught = throws_analysis::find_uncaught_throw_types_with_context(
        content,
        info.docblock_position,
        Some(&ThrowsContext {
            class_loader,
            resolved_class_loader,
            function_loader,
            use_map,
            local_classes,
            backend,
        }),
    );
    let existing_fqns: Vec<String> = info
        .doc_throws
        .iter()
        .map(|t| {
            resolve_type_name_to_fqn(t, use_map, file_namespace, resolved_class_loader)
                .to_lowercase()
        })
        .collect();
    let missing_throws = uncaught
        .iter()
        // The throws analysis already resolved it.
        .map(|exc| exc.to_string())
        .filter(|exc| !existing_fqns.contains(&exc.to_lowercase()))
        .collect();

    BodyFacts {
        enriched_return,
        missing_throws,
    }
}

/// Check whether the docblock needs updating.
pub(super) fn check_needs_update(
    info: &FunctionWithDocblock,
    facts: &BodyFacts,
    class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>,
    use_map: &HashMap<String, String>,
    file_namespace: &Option<String>,
) -> bool {
    // When the docblock already has at least one @param tag the user has
    // opted-in to documenting parameters, so every signature param is
    // relevant.  When the docblock has *zero* @param tags we only consider
    // params that need enrichment (matching generate-docblock behaviour).
    let has_any_doc_params = !info.doc_params.is_empty();

    if has_any_doc_params {
        // Check for missing, extra, or reordered params.
        let doc_names = info.doc_params.iter().map(DocParam::bare_name);
        let sig_names = info.sig_params.iter().map(|p| p.name.as_str());
        if !doc_names.eq(sig_names) {
            return true;
        }
    } else {
        // No @param tags at all — only flag if a param needs enrichment.
        let needs_enrichment = info.sig_params.iter().any(|sp| {
            enrichment_plain(sp.type_hint.as_ref(), class_loader, use_map, file_namespace).is_some()
        });
        if needs_enrichment {
            return true;
        }
    }

    // Check for type contradictions in @param tags.
    for sig_param in &info.sig_params {
        if let Some(native_type) = &sig_param.type_hint
            && let Some(doc_param) = info.doc_param(&sig_param.name)
            && is_type_contradiction(&doc_param.type_parsed, native_type)
        {
            return true;
        }
    }

    // Check whether any existing @param type needs enrichment (e.g. a bare
    // `Closure` that should become `(Closure(): mixed)`, or a class with templates).
    // Skip when the doc type is already more specific (contains `<` or `(`).
    for sig_param in &info.sig_params {
        if let Some(doc_param) = info.doc_param(&sig_param.name) {
            // If the doc type already carries generic params or a callable
            // signature, it is already enriched — no update needed.
            if doc_param.type_parsed.has_type_structure() {
                continue;
            }
            if let Some(enriched) = enrichment_plain_typed(
                sig_param.type_hint.as_ref(),
                class_loader,
                use_map,
                file_namespace,
            ) && !enriched.equivalent(&doc_param.type_parsed)
            {
                return true;
            }
        }
    }

    // Check @return tag.
    if let Some(sig_ret) = &info.sig_return
        && let Some(doc_ret) = &info.doc_return
    {
        // Remove `@return void` if the signature also has `: void`.
        if sig_ret.is_void() && doc_ret.type_parsed.is_void() {
            return true;
        }
        if is_type_contradiction(&doc_ret.type_parsed, sig_ret) {
            return true;
        }
    }

    facts.enriched_return.is_some() || !facts.missing_throws.is_empty()
}

/// Check if a docblock type contradicts a native type hint.
///
/// A contradiction means the docblock type is NOT a refinement of the native
/// type. For example, docblock says `string` but native says `int` is a
/// contradiction. But docblock says `non-empty-string` while native says
/// `string` is a refinement (not a contradiction).
pub(super) fn is_type_contradiction(doc_type: &PhpType, native_type: &PhpType) -> bool {
    // `PhpType::equivalent` handles `?T` ↔ `T|null`, order-independent
    // unions, and FQN shortening.  It does not do case-insensitive
    // comparison, but PHP class names in practice come from the same
    // source so casing matches.
    if doc_type.equivalent(native_type) {
        return false;
    }

    // Check whether the docblock type is a compatible refinement of the
    // native type (e.g. `class-string<Foo>` refines `string`,
    // `list<User>` refines `array`, `positive-int` refines `int`).
    // This uses the shared refinement checker that also guards
    // `resolve_effective_type`.
    let native_core = native_type
        .non_null_type()
        .unwrap_or_else(|| native_type.clone());
    let doc_core = doc_type.non_null_type().unwrap_or_else(|| doc_type.clone());
    if is_compatible_refinement_typed(&doc_core, &native_core) {
        return false;
    }

    // For single-member types, compare base names directly.  If they
    // differ and neither is a refinement of the other, it is a
    // contradiction.
    let doc_bases = doc_type.union_members();
    let native_bases = native_type.union_members();

    if doc_bases.len() == 1
        && native_bases.len() == 1
        && !doc_bases[0].equivalent(native_bases[0])
        && !is_compatible_refinement_typed(doc_bases[0], native_bases[0])
    {
        return true;
    }

    false
}

/// Check if the `@return` tag should be removed.
pub(super) fn should_remove_return(info: &FunctionWithDocblock) -> bool {
    if let Some(sig_ret) = &info.sig_return
        && let Some(doc_ret) = &info.doc_return
        && sig_ret.is_void()
        && doc_ret.type_parsed.is_void()
        && doc_ret.description.is_empty()
    {
        return true;
    }
    false
}

/// Check if the `@return` tag needs its type updated.
pub(super) fn should_update_return(info: &FunctionWithDocblock) -> bool {
    if let Some(sig_ret) = &info.sig_return
        && let Some(doc_ret) = &info.doc_return
        && is_type_contradiction(&doc_ret.type_parsed, sig_ret)
    {
        return true;
    }
    false
}
