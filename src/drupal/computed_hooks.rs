//! The core methods that fire hooks under computed names, ported from
//! PHPlumb: an implementation of `hook_form_FORM_ID_alter` is fired by the
//! `alter()` call `FormBuilder::prepareForm()` makes with the name it
//! builds, which nothing in the call itself spells.
//!
//! The table is tied to core's internals, so it is kept apart from the rest
//! of the hook support: dropping this module leaves every other hook
//! feature working.

use crate::Backend;

/// Which entity types an `ENTITY_TYPE` placeholder of a row stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Storage {
    Any,
    /// Entity types whose storage is not a config entity storage.
    Content,
    /// Entity types stored by a config entity storage.
    Config,
}

/// Each row: the computed hook names, the core methods that fire them, and
/// the entity types an `ENTITY_TYPE` placeholder stands for.
pub(crate) const COMPUTED: &[(&[&str], &[&str], Storage)] = &[
    (
        &[
            "ENTITY_TYPE_create",
            "ENTITY_TYPE_presave",
            "ENTITY_TYPE_insert",
            "ENTITY_TYPE_update",
            "ENTITY_TYPE_predelete",
            "ENTITY_TYPE_delete",
            "ENTITY_TYPE_revision_delete",
            "ENTITY_TYPE_translation_create",
            "ENTITY_TYPE_translation_insert",
            "ENTITY_TYPE_translation_delete",
            "ENTITY_TYPE_field_values_init",
        ],
        &["Drupal\\Core\\Entity\\EntityStorageBase::invokeHook"],
        Storage::Content,
    ),
    (
        &[
            "ENTITY_TYPE_create",
            "ENTITY_TYPE_presave",
            "ENTITY_TYPE_insert",
            "ENTITY_TYPE_update",
            "ENTITY_TYPE_predelete",
            "ENTITY_TYPE_delete",
            "ENTITY_TYPE_revision_delete",
            "ENTITY_TYPE_translation_create",
            "ENTITY_TYPE_translation_insert",
            "ENTITY_TYPE_translation_delete",
            "ENTITY_TYPE_field_values_init",
        ],
        &["Drupal\\Core\\Config\\Entity\\ConfigEntityStorage::invokeHook"],
        Storage::Config,
    ),
    (
        &[
            "entity_create",
            "entity_presave",
            "entity_insert",
            "entity_update",
            "entity_predelete",
            "entity_delete",
            "entity_revision_delete",
            "entity_translation_create",
            "entity_translation_insert",
            "entity_translation_delete",
            "entity_field_values_init",
        ],
        &[
            "Drupal\\Core\\Entity\\EntityStorageBase::invokeHook",
            "Drupal\\Core\\Config\\Entity\\ConfigEntityStorage::invokeHook",
        ],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_load"],
        &["Drupal\\Core\\Entity\\EntityStorageBase::postLoad"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_storage_load"],
        &["Drupal\\Core\\Entity\\ContentEntityStorageBase::invokeStorageLoadHook"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_revision_create"],
        &["Drupal\\Core\\Entity\\ContentEntityStorageBase::createRevision"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_duplicate"],
        &[
            "Drupal\\Core\\Entity\\EntityBase::createDuplicate",
            "Drupal\\Core\\Entity\\ContentEntityBase::createDuplicate",
        ],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_access"],
        &["Drupal\\Core\\Entity\\EntityAccessControlHandler::access"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_create_access"],
        &["Drupal\\Core\\Entity\\EntityAccessControlHandler::createAccess"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_view", "ENTITY_TYPE_view_alter"],
        &["Drupal\\Core\\Entity\\EntityViewBuilder::buildMultiple"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_build_defaults_alter"],
        &["Drupal\\Core\\Entity\\EntityViewBuilder::viewMultiple"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_view_mode_alter"],
        &["Drupal\\Core\\Entity\\EntityViewBuilder::getBuildDefaults"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_form_mode_alter"],
        &["Drupal\\Core\\Entity\\Entity\\EntityFormDisplay::collectRenderDisplay"],
        Storage::Any,
    ),
    (
        &["ENTITY_TYPE_prepare_form"],
        &["Drupal\\Core\\Entity\\EntityForm::prepareInvokeAll"],
        Storage::Any,
    ),
    (
        &[
            "entity_query_ENTITY_TYPE_alter",
            "entity_query_tag__TAG_alter",
            "entity_query_tag__ENTITY_TYPE__TAG_alter",
        ],
        &["Drupal\\Core\\Entity\\Query\\QueryBase::alter"],
        Storage::Any,
    ),
    (
        &[
            "form_alter",
            "form_FORM_ID_alter",
            "form_BASE_FORM_ID_alter",
        ],
        &["Drupal\\Core\\Form\\FormBuilder::prepareForm"],
        Storage::Any,
    ),
    (
        &["query_alter", "query_TAG_alter"],
        &[
            "Drupal\\Core\\Database\\Query\\Select::preExecute",
            "Drupal\\Core\\Entity\\Plugin\\EntityReferenceSelection\\DefaultSelection::reAlterQuery",
        ],
        Storage::Any,
    ),
    (
        &["field_widget_single_element_WIDGET_TYPE_form_alter"],
        &["Drupal\\Core\\Field\\WidgetBase::formSingleElement"],
        Storage::Any,
    ),
    (
        &["field_widget_complete_WIDGET_TYPE_form_alter"],
        &["Drupal\\Core\\Field\\WidgetBase::form"],
        Storage::Any,
    ),
    (
        &["block_view_BASE_BLOCK_ID_alter"],
        &["Drupal\\block\\BlockViewBuilder::buildPreRenderableBlock"],
        Storage::Any,
    ),
    (
        &["block_build_BASE_BLOCK_ID_alter"],
        &["Drupal\\block\\BlockViewBuilder::viewMultiple"],
        Storage::Any,
    ),
    (
        &[
            "theme_suggestions_HOOK",
            "theme_suggestions_alter",
            "theme_suggestions_HOOK_alter",
        ],
        &["Drupal\\Core\\Theme\\ThemeManager::buildThemeHookSuggestions"],
        Storage::Any,
    ),
    (
        &["preprocess_HOOK"],
        &["Drupal\\Core\\Theme\\Registry::collectModulePreprocess"],
        Storage::Any,
    ),
    (
        &[
            "plugin_filter_TYPE_alter",
            "plugin_filter_TYPE__CONSUMER_alter",
        ],
        &["Drupal\\Core\\Plugin\\FilteredPluginManagerTrait::getFilteredDefinitions"],
        Storage::Any,
    ),
    (
        &[
            "language_fallback_candidates_alter",
            "language_fallback_candidates_OPERATION_alter",
        ],
        &["Drupal\\language\\ConfigurableLanguageManager::getFallbackCandidates"],
        Storage::Any,
    ),
    (
        &["migrate_MIGRATION_ID_prepare_row"],
        &["Drupal\\migrate\\Plugin\\migrate\\source\\SourcePluginBase::prepareRow"],
        Storage::Any,
    ),
    (
        &["jsonapi_ENTITY_TYPE_filter_access"],
        &["Drupal\\jsonapi\\Access\\TemporaryQueryGuard::getAccessResultsFromEntityFilterHook"],
        Storage::Any,
    ),
];

/// Whether `class`, a resolved class name, declares one of the table's core
/// methods.
pub(crate) fn is_computing_class(class: &str) -> bool {
    let class = class.trim_start_matches('\\');
    COMPUTED.iter().any(|(_, sites, _)| {
        sites.iter().any(|site| {
            site.split_once("::")
                .is_some_and(|(site_class, _)| site_class.eq_ignore_ascii_case(class))
        })
    })
}

/// Whether `class::method`, a resolved class name and a method, is one of
/// the table's core methods.
pub(crate) fn is_computing_method(class: &str, method: &str) -> bool {
    let class = class.trim_start_matches('\\');
    COMPUTED.iter().any(|(_, sites, _)| {
        sites.iter().any(|site| {
            site.split_once("::")
                .is_some_and(|(site_class, site_method)| {
                    site_class.eq_ignore_ascii_case(class)
                        && site_method.eq_ignore_ascii_case(method)
                })
        })
    })
}

/// Whether `text` matches `shape`, where each `*` stands for a non-empty
/// run.
fn shape_matches(shape: &str, text: &str) -> bool {
    let Some((head, tail)) = shape.split_once('*') else {
        return shape == text;
    };
    let Some(after) = text.strip_prefix(head) else {
        return false;
    };
    (1..=after.len())
        .filter(|length| after.is_char_boundary(*length))
        .any(|length| shape_matches(tail, &after[length..]))
}

impl Backend {
    /// Whether the entity type `id` is declared, and stored the way
    /// `storage` asks: by a config entity storage or not.
    fn is_entity_type_of(&self, id: &str, storage: Storage) -> bool {
        const CONFIG_STORAGE: &str = "Drupal\\Core\\Config\\Entity\\ConfigEntityStorage";
        let declared = self.drupal_entity_types(id);
        if storage == Storage::Any {
            return !declared.is_empty();
        }
        let loader = self.resolved_class_loader_with(&[]);
        declared.iter().any(|(_, handler)| {
            let is_config = handler.is_some_and(|handler| {
                self.find_or_load_class(handler.as_str())
                    .is_some_and(|class| {
                        crate::class_lookup::is_subtype_of(&class, CONFIG_STORAGE, &loader)
                    })
            });
            is_config == (storage == Storage::Config)
        })
    }

    /// The calls in core that fire the computed hook `hook`, implemented by
    /// a theme when `is_theme`: for each of the table's methods that builds
    /// a name `hook` matches, the invoking calls there of the
    /// implementation's group whose built name matches it with the most
    /// written-out characters, as files and byte ranges.
    pub(crate) fn computed_hook_call_sites(
        &self,
        hook: &str,
        is_theme: bool,
    ) -> Vec<(String, u32, u32)> {
        let mut found = Vec::new();
        for (patterns, sites, storage) in COMPUTED {
            let is_entity_type = |id: &str| self.is_entity_type_of(id, *storage);
            let matches = patterns
                .iter()
                .any(|pattern| super::hooks::matches_hook_pattern(pattern, hook, &is_entity_type));
            if !matches {
                continue;
            }
            for site in *sites {
                let Some((class, method)) = site.split_once("::") else {
                    continue;
                };
                // Loading the class parses its file, which records the calls.
                if self.find_or_load_class(class).is_none() {
                    continue;
                }
                let Some(uri) = self.class_declaration_uri(class) else {
                    continue;
                };
                let Some(map) = self.symbol_map_for(&uri) else {
                    continue;
                };
                let scored: Vec<(usize, u32, u32)> = map
                    .drupal_computed_hooks
                    .iter()
                    .filter(|call| call.method.eq_ignore_ascii_case(method))
                    .filter(|call| call.alter == hook.ends_with("_alter"))
                    .filter(|call| call.themes.is_none_or(|themes| themes == is_theme))
                    .filter(|call| call.shape.contains('*') && shape_matches(&call.shape, hook))
                    .map(|call| {
                        let score = call.shape.len() - call.shape.matches('*').count();
                        (score, call.start, call.end)
                    })
                    .collect();
                let Some(best) = scored.iter().map(|(score, _, _)| *score).max() else {
                    continue;
                };
                for (_, start, end) in scored.into_iter().filter(|(score, _, _)| *score == best) {
                    if !found.contains(&(uri.clone(), start, end)) {
                        found.push((uri.clone(), start, end));
                    }
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_match_with_a_non_empty_run_per_star() {
        assert!(shape_matches("form_*_alter", "form_user_login_alter"));
        assert!(shape_matches("*_alter", "form_alter"));
        assert!(!shape_matches("form_*_alter", "form__alter"));
        assert!(!shape_matches("form_*_alter", "query_x_alter"));
    }

    #[test]
    fn computing_methods_are_matched_by_class_and_method() {
        assert!(is_computing_method(
            "\\Drupal\\Core\\Form\\FormBuilder",
            "prepareForm"
        ));
        assert!(!is_computing_method(
            "Drupal\\Core\\Form\\FormBuilder",
            "buildForm"
        ));
    }
}
