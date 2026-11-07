//! The variables a Drupal theme hook's Twig templates receive.
//!
//! A module's template has no Twig template name a render could key its
//! context by, and a render array names a hook rather than a template, so a
//! template finds its variables through the hooks that claim it (see
//! [`Backend::drupal_template_theme_hooks`]): each hook's declared variables
//! or render element, the variables Drupal adds to every template, and the
//! `#` keys of the render arrays whose `#theme` names the hook or the
//! template's own suggestion.  A hook's own templates also take the render
//! arrays of a suggestion of it no theme writes a template for, which
//! Drupal draws with them.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use crate::Backend;
use crate::reference_index::ReferenceIndexKey;
use crate::symbol_map::{StringKeyKind, SymbolKind};
use crate::twig::variables::{Origin, OriginKind, OriginValue};

const ATTRIBUTE_CLASS: &str = "\\Drupal\\Core\\Template\\Attribute";

/// The variables Drupal adds to every template a theme hook draws, with
/// what each holds.
const DEFAULT_VARIABLES: [(&str, &str); 11] = [
    (
        "attributes",
        "The HTML attributes of the template's outer element",
    ),
    (
        "title_attributes",
        "The HTML attributes of the template's title",
    ),
    (
        "content_attributes",
        "The HTML attributes of the template's content",
    ),
    ("title_prefix", "Render arrays modules add before the title"),
    ("title_suffix", "Render arrays modules add after the title"),
    ("db_is_active", "Whether the database is available"),
    ("is_admin", "Whether the current user is an administrator"),
    ("logged_in", "Whether the current user is logged in"),
    ("user", "The current user"),
    ("directory", "The directory of the active theme"),
    (
        "theme_hook_original",
        "The theme hook the template was rendered for, before suggestions",
    ),
];

/// The type a variable Drupal adds to every template holds.
fn default_variable_type(name: &str) -> Option<&'static str> {
    Some(match name {
        "attributes" | "title_attributes" | "content_attributes" => ATTRIBUTE_CLASS,
        "title_prefix" | "title_suffix" => "array",
        "db_is_active" | "is_admin" | "logged_in" => "bool",
        "user" => "\\Drupal\\Core\\Session\\AccountInterface",
        "directory" | "theme_hook_original" => "string",
        _ => return None,
    })
}

impl Backend {
    /// Whether some theme hook claims the template at `uri`.
    fn is_drupal_hook_template(&self, uri: &str) -> bool {
        self.is_drupal_project()
            && crate::util::uri_to_path(uri)
                .is_some_and(|path| !self.drupal_template_theme_hooks(&path).is_empty())
    }

    /// The variables Drupal adds to the template at `uri`: every one of
    /// them when a theme hook claims it, else none.
    pub(crate) fn drupal_template_default_variables(
        &self,
        uri: &str,
    ) -> &'static [(&'static str, &'static str)] {
        if self.is_drupal_hook_template(uri) {
            &DEFAULT_VARIABLES
        } else {
            &[]
        }
    }

    /// The type of the variable `name` Drupal adds to the template at
    /// `uri`.  Drupal turns whatever a render passes as one of the
    /// attributes variables into an `Attribute`, so those hold one even
    /// when a hook declares them.
    pub(crate) fn drupal_template_default_type(
        &self,
        uri: &str,
        name: &str,
    ) -> Option<&'static str> {
        default_variable_type(name).filter(|_| self.is_drupal_hook_template(uri))
    }

    /// The `Attribute` class, when `name` is an attributes variable of the
    /// Drupal template at `uri` (see [`Self::drupal_template_default_type`]).
    pub(crate) fn drupal_template_attribute_type(
        &self,
        uri: &str,
        name: &str,
    ) -> Option<&'static str> {
        self.drupal_template_default_type(uri, name)
            .filter(|ty| *ty == ATTRIBUTE_CLASS)
    }

    /// Add to `origins` the variables the theme hooks that claim the
    /// template at `uri` hand it: each hook's declared variables or render
    /// element, and those of them the render arrays naming the hook pass.
    pub(crate) fn collect_drupal_template_received(&self, uri: &str, origins: &mut Vec<Origin>) {
        if !self.is_drupal_project() {
            return;
        }
        let Some(path) = crate::util::uri_to_path(uri) else {
            return;
        };
        let hooks = self.drupal_template_theme_hooks(&path);
        if hooks.is_empty() {
            return;
        }
        let mut names: Vec<String> = Vec::new();
        let mut variables: HashSet<String> = HashSet::new();
        for hook in &hooks {
            if !names.iter().any(|name| name == hook.name()) {
                names.push(hook.name().to_string());
            }
            let source = self.drupal_theme_hook_variable_source(hook);
            let declared = source
                .entry
                .render_element
                .iter()
                .chain(&source.entry.variables);
            for variable in declared {
                origins.push(Origin {
                    name: variable.name.clone(),
                    kind: OriginKind::ThemeHook {
                        hook: hook.name().to_string(),
                    },
                    uri: source.uri.clone(),
                    start: variable.start,
                    end: variable.end,
                    value: OriginValue::Unknown,
                });
            }
            // A render element hook takes the `#` keys into its element.
            if source.entry.render_element.is_none() {
                variables.extend(
                    source
                        .entry
                        .variables
                        .iter()
                        .map(|variable| variable.name.clone()),
                );
            }
        }
        let own = crate::drupal::theme_hooks::template_key(&path).unwrap_or_default();
        for hook in &hooks {
            let is_suggestion_template = own
                .strip_prefix(hook.name())
                .is_some_and(|rest| rest.starts_with("__"));
            if !is_suggestion_template {
                for suggestion in self.drupal_unclaimed_suggestions(hook.name()) {
                    if !names.contains(&suggestion) {
                        names.push(suggestion);
                    }
                }
            }
        }
        // A theme's template for a suggestion is rendered for the
        // suggestion's own name as well.
        if !own.is_empty() && !names.contains(&own) {
            names.push(own);
        }
        if variables.is_empty() {
            return;
        }
        let keys: Vec<ReferenceIndexKey> = names
            .iter()
            .map(|name| ReferenceIndexKey::StringKey {
                kind: StringKeyKind::DrupalThemeHook,
                key: name.clone(),
            })
            .collect();
        let mut snapshot = self.user_file_symbol_maps_for_reference_keys(&keys);
        snapshot.sort_by(|a, b| a.0.cmp(&b.0));
        for (file_uri, map) in snapshot {
            if map.twig_contexts.is_empty() {
                continue;
            }
            let starts: Vec<u32> = map
                .spans
                .iter()
                .filter(|span| {
                    matches!(
                        &span.kind,
                        SymbolKind::StringKey {
                            kind: StringKeyKind::DrupalThemeHook,
                            key,
                            is_write: false,
                            ..
                        } if names.contains(key)
                    )
                })
                .map(|span| span.start)
                .collect();
            if starts.is_empty() {
                continue;
            }
            self.collect_render_site_origins(
                &file_uri,
                &map,
                &starts,
                |name| variables.contains(name),
                origins,
            );
        }
    }

    /// The suggestions of the hook `hook` render arrays name that fall back
    /// to it, and for which no theme writes a template.
    fn drupal_unclaimed_suggestions(&self, hook: &str) -> Vec<String> {
        let suggestions = self.drupal_theme_suggestions();
        let prefix = format!("{hook}__");
        let candidates: Vec<&String> = suggestions
            .range::<str, _>((
                std::ops::Bound::Included(prefix.as_str()),
                std::ops::Bound::Unbounded,
            ))
            .take_while(|suggestion| suggestion.starts_with(&prefix))
            .collect();
        if candidates.is_empty() {
            return Vec::new();
        }
        let templates = self.drupal.templates.read();
        let extensions = self.drupal.extensions.read();
        candidates
            .into_iter()
            .filter(|suggestion| {
                !templates
                    .get(suggestion.as_str())
                    .is_some_and(|paths| paths.iter().any(|path| extensions.owner_is_theme(path)))
            })
            .filter(|suggestion| {
                self.drupal_theme_hooks(suggestion)
                    .first()
                    .is_some_and(|declared| declared.name() == hook)
            })
            .cloned()
            .collect()
    }

    /// The suggestions render arrays name, read from the reference index
    /// once and added to as files name new ones (see
    /// [`Self::note_drupal_theme_suggestions`]).
    fn drupal_theme_suggestions(&self) -> Arc<BTreeSet<String>> {
        if let Some(suggestions) = self.drupal.theme_suggestions.read().as_ref() {
            return Arc::clone(suggestions);
        }
        self.ensure_workspace_index_ready_for_request();
        let Some(keys) = self.indexed_string_keys(StringKeyKind::DrupalThemeHook) else {
            return Arc::default();
        };
        let suggestions: Arc<BTreeSet<String>> =
            Arc::new(keys.into_iter().filter(|key| key.contains("__")).collect());
        *self.drupal.theme_suggestions.write() = Some(Arc::clone(&suggestions));
        suggestions
    }

    /// Add the suggestions `map` names that [`Self::drupal_theme_suggestions`]
    /// does not hold yet.  One no file names any more stays, and reads
    /// nothing.
    pub(crate) fn note_drupal_theme_suggestions(&self, map: &crate::symbol_map::SymbolMap) {
        let Some(known) = self.drupal.theme_suggestions.read().clone() else {
            return;
        };
        let new: Vec<&String> = map
            .spans
            .iter()
            .filter_map(|span| match &span.kind {
                SymbolKind::StringKey {
                    kind: StringKeyKind::DrupalThemeHook,
                    key,
                    ..
                } if key.contains("__") && !known.contains(key) => Some(key),
                _ => None,
            })
            .collect();
        if new.is_empty() {
            return;
        }
        let mut suggestions = (*known).clone();
        suggestions.extend(new.into_iter().cloned());
        *self.drupal.theme_suggestions.write() = Some(Arc::new(suggestions));
    }
}
