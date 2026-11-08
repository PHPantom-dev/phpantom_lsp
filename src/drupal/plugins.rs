//! Drupal's plugins: the plugin types the project's plugin managers
//! declare, and the plugin classes of each type.
//!
//! No table of plugin types is kept.  A plugin manager extends
//! `DefaultPluginManager`, and its constructor passes the plugin
//! subdirectory, the attribute class, and the annotation class to the
//! parent's as literals; that call is read from each class (see
//! `symbol_map::extraction::drupal::emit_drupal_plugin_manager`).  The
//! plugins of a type are the classes in that subdirectory of every module
//! namespace and of every `Drupal\Core\*` and `Drupal\Component\*`
//! namespace, with the type's attribute or, failing that, its annotation,
//! the way Drupal's discovery finds them.
//!
//! A plugin ID is a [`StringKeyKind::DrupalPlugin`] key, keyed by its
//! manager as well (see [`plugin_key`]): the same ID means a field type to
//! one manager and a widget to another.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tower_lsp_server::gen_lsp_types::Location;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::symbol_map::{DRUPAL_PLUGIN_MANAGER, SymbolMap};
use crate::types::ClassInfo;

const DEFAULT_MANAGER: &str = "Drupal\\Core\\Plugin\\DefaultPluginManager";
const ATTRIBUTE_INTERFACE: &str = "Drupal\\Component\\Plugin\\Attribute\\AttributeInterface";
/// The annotation a manager whose constructor names none reads.
const DEFAULT_ANNOTATION: &str = "Drupal\\Component\\Annotation\\Plugin";
const FIELD_TYPES: &str = "Drupal\\Core\\Field\\FieldTypePluginManager";
const WIDGETS: &str = "Drupal\\Core\\Field\\WidgetPluginManager";
const FORMATTERS: &str = "Drupal\\Core\\Field\\FormatterPluginManager";
pub(crate) const CONSTRAINTS: &str = "Drupal\\Core\\Validation\\ConstraintManager";
const BASE_FIELD: &str = "Drupal\\Core\\Field\\BaseFieldDefinition";

/// The key of a [`StringKeyKind::DrupalPlugin`](crate::symbol_map::StringKeyKind::DrupalPlugin)
/// span: the plugin ID `id` of the manager `manager`.
pub(crate) fn plugin_key(manager: &str, id: &str) -> String {
    format!("{manager}#{id}")
}

/// The manager and the ID of a plugin ID's span key.
pub(crate) fn split_plugin_key(key: &str) -> (&str, &str) {
    key.split_once('#').unwrap_or(("", key))
}

/// The manager whose plugins the `type` of a field's display options names
/// for the display `display`: widgets for `form`, formatters for `view`.
pub(crate) fn display_manager(display: &str) -> Option<&'static str> {
    match display {
        "form" => Some(WIDGETS),
        "view" => Some(FORMATTERS),
        _ => None,
    }
}

/// The manager whose plugin ID a static call of `method` on `class`, a
/// resolved class name, takes first: `BaseFieldDefinition::create()` takes
/// a field type.
pub(crate) fn static_call_manager(class: &str, method: &str) -> Option<&'static str> {
    (class
        .trim_start_matches('\\')
        .eq_ignore_ascii_case(BASE_FIELD)
        && method.eq_ignore_ascii_case("create"))
    .then_some(FIELD_TYPES)
}

/// The bases of a derivative ID, longest first: `a:b:c` gives `a:b`, then
/// `a`.  A deriver makes the derivatives of a base plugin at run time, so
/// only the base is declared.
pub(crate) fn derivative_bases(id: &str) -> impl Iterator<Item = &str> {
    id.rmatch_indices(':').map(move |(at, _)| &id[..at])
}

/// One plugin type: a manager, the subdirectory its plugins live in, and
/// what they declare themselves with.
#[derive(Debug, Clone)]
pub(crate) struct PluginType {
    pub manager: Atom,
    /// The subdirectory as a namespace, lowercased: `plugin\block`.
    namespace: String,
    attribute: Option<Atom>,
    annotation: Option<Atom>,
}

/// One plugin of a type.
#[derive(Debug, Clone)]
pub(crate) struct Plugin {
    pub id: String,
    pub class: Atom,
}

/// The plugin types and the plugins of each, read from every declaring
/// file at once.
#[derive(Default)]
pub(crate) struct PluginIndex {
    types: Vec<PluginType>,
    /// The plugins of each manager, by the manager's lowercased name, in a
    /// stable order.
    by_manager: HashMap<String, Vec<Plugin>>,
    /// The files whose plugin managers or plugin declarations were read,
    /// which a change to invalidates the index.
    sources: HashSet<String>,
}

impl PluginIndex {
    /// The plugins of `manager`.
    pub(crate) fn plugins_of(&self, manager: &str) -> &[Plugin] {
        self.by_manager
            .get(&manager.to_ascii_lowercase())
            .map_or(&[], Vec::as_slice)
    }

    /// The plugins of `manager` named `id`; for a derivative ID with none,
    /// those named after its longest base that has some.
    pub(crate) fn named(&self, manager: &str, id: &str) -> Vec<&Plugin> {
        let plugins = self.plugins_of(manager);
        let exact: Vec<&Plugin> = plugins.iter().filter(|plugin| plugin.id == id).collect();
        if !exact.is_empty() {
            return exact;
        }
        derivative_bases(id)
            .map(|base| -> Vec<&Plugin> {
                plugins.iter().filter(|plugin| plugin.id == base).collect()
            })
            .find(|found| !found.is_empty())
            .unwrap_or_default()
    }

    /// Whether `manager` declares a plugin type.
    fn is_manager(&self, class: &str) -> Option<Atom> {
        self.types
            .iter()
            .find(|plugin_type| plugin_type.manager.eq_ignore_ascii_case(class))
            .map(|plugin_type| plugin_type.manager)
    }
}

/// Whether the symbol map holds anything the plugin index reads.
fn feeds_index(map: &SymbolMap) -> bool {
    !map.drupal_plugins.is_empty() || !map.drupal_plugin_managers.is_empty()
}

/// The part of a plugin class's name below the namespace its extension
/// registers for discovery, lowercased: what follows `Drupal\<module>\`,
/// or `Drupal\Core\<component>\` and `Drupal\Component\<component>\`.
fn discovery_relative_name(class: &str) -> Option<String> {
    let lower = class.to_ascii_lowercase();
    let (extension, rest) = lower.strip_prefix("drupal\\")?.split_once('\\')?;
    let rest = if extension == "core" || extension == "component" {
        rest.split_once('\\')?.1
    } else {
        rest
    };
    Some(rest.to_string())
}

/// The short name of a class or an annotation, after its last `\`.
fn short_name(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

impl Backend {
    /// The plugin types and their plugins, read once and kept until a file
    /// that declares a manager or a plugin changes.
    pub(crate) fn drupal_plugin_index(&self) -> Arc<PluginIndex> {
        if let Some(index) = self.drupal.plugin_index.read().as_ref() {
            return Arc::clone(index);
        }
        let generation = self.drupal.plugin_generation.load(Ordering::Acquire);
        let index = Arc::new(self.read_drupal_plugin_index());
        let mut slot = self.drupal.plugin_index.write();
        // A file changed while the index was read: it is answered from, but
        // read again next time.
        if self.drupal.plugin_generation.load(Ordering::Acquire) == generation {
            *slot = Some(Arc::clone(&index));
        }
        index
    }

    /// Drop the plugin index when the file at `uri`, whose symbol map is
    /// now `map` (`None` once it is gone), fed it or now would.
    pub(crate) fn note_drupal_plugin_sources(&self, uri: &str, map: Option<&SymbolMap>) {
        let feeds = map.is_none_or(feeds_index);
        let stale = feeds
            || self
                .drupal
                .plugin_index
                .read()
                .as_ref()
                .is_some_and(|index| index.sources.contains(uri));
        if stale {
            self.drupal.plugin_generation.fetch_add(1, Ordering::AcqRel);
            let mut slot = self.drupal.plugin_index.write();
            if slot.is_some() {
                *slot = None;
            }
        }
    }

    /// Read the plugin types from the project's plugin managers, then the
    /// plugins of each from its classes.
    fn read_drupal_plugin_index(&self) -> PluginIndex {
        let mut index = PluginIndex::default();
        let mut managers = Vec::new();
        let mut declarations = Vec::new();
        self.visit_declaring_symbol_maps(|uri, map| {
            if !feeds_index(map) {
                return;
            }
            index.sources.insert(uri.to_string());
            managers.extend(map.drupal_plugin_managers.iter().cloned());
            declarations.extend(map.drupal_plugins.iter().cloned());
        });
        let loader = self.resolved_class_loader_with(&[]);
        let mut subtypes: HashMap<(Atom, &str), bool> = HashMap::new();
        let mut is_a = |class: Atom, ancestor: &'static str| {
            *subtypes.entry((class, ancestor)).or_insert_with(|| {
                loader(Fqn::new(&class)).is_some_and(|info| is_subtype_of(&info, ancestor, &loader))
            })
        };
        for site in managers {
            if !is_a(site.class, DEFAULT_MANAGER) {
                continue;
            }
            // As the default manager's constructor reads it: a fifth class
            // that is an attribute is one, and the sixth the annotation;
            // anything else in the fifth place is the annotation of a
            // manager that reads annotations only.
            let (attribute, annotation) = match site.fifth {
                Some(fifth) if self.is_plugin_attribute(fifth, &loader) => {
                    (Some(fifth), site.sixth)
                }
                fifth => (
                    None,
                    Some(fifth.unwrap_or_else(|| crate::atom::atom(DEFAULT_ANNOTATION))),
                ),
            };
            index.types.push(PluginType {
                manager: site.class,
                namespace: site
                    .subdir
                    .trim_matches('/')
                    .replace('/', "\\")
                    .to_ascii_lowercase(),
                attribute,
                annotation,
            });
        }
        index.types.sort_by_key(|a| a.manager);
        index
            .types
            .dedup_by(|a, b| a.manager.eq_ignore_ascii_case(&b.manager));

        // An attribute that extends a type's attribute declares a plugin of
        // the type too, as `#[FormElement]` does a render element.
        let mut extends: HashMap<(Atom, Atom), bool> = HashMap::new();
        let mut attribute_matches = |written: Atom, target: Atom| {
            written.eq_ignore_ascii_case(&target)
                || *extends.entry((written, target)).or_insert_with(|| {
                    loader(Fqn::new(&written))
                        .is_some_and(|info| is_subtype_of(&info, &target, &loader))
                })
        };
        for group in declarations.chunk_by(|a, b| a.class == b.class) {
            let class = group[0].class;
            let Some(relative) = discovery_relative_name(&class) else {
                continue;
            };
            for plugin_type in &index.types {
                let in_subdir = relative
                    .strip_prefix(plugin_type.namespace.as_str())
                    .is_some_and(|below| below.starts_with('\\'));
                if !in_subdir {
                    continue;
                }
                let by_attribute = plugin_type.attribute.and_then(|target| {
                    group
                        .iter()
                        .find(|site| !site.is_annotation && attribute_matches(site.marker, target))
                });
                let declared = by_attribute.or_else(|| {
                    let target = plugin_type.annotation?;
                    group.iter().find(|site| {
                        site.is_annotation
                            && short_name(&site.marker).eq_ignore_ascii_case(short_name(&target))
                    })
                });
                if let Some(site) = declared {
                    index
                        .by_manager
                        .entry(plugin_type.manager.to_ascii_lowercase())
                        .or_default()
                        .push(Plugin {
                            id: site.id.clone(),
                            class,
                        });
                }
            }
        }
        index
    }

    /// Whether the class a plugin manager names fifth is an attribute: it
    /// implements the plugin attribute interface, or, when the project
    /// lacks it, its namespace is not an `Annotation` one.
    fn is_plugin_attribute(&self, class: Atom, loader: &ResolvedClassLoader<'_>) -> bool {
        match loader(Fqn::new(&class)) {
            Some(info) => is_subtype_of(&info, ATTRIBUTE_INTERFACE, loader),
            None => !class.to_ascii_lowercase().contains("\\annotation\\"),
        }
    }

    /// The plugin manager a receiver of the classes `classes` is: a manager
    /// itself, a class that extends one, or an interface or parent class
    /// exactly one manager has.  `None` when that is not one manager.
    pub(crate) fn drupal_plugin_manager_of(
        &self,
        classes: &[String],
        loader: &ResolvedClassLoader<'_>,
    ) -> Option<Atom> {
        let index = self.drupal_plugin_index();
        let mut found: Option<Atom> = None;
        for class in classes {
            let class = class.trim_start_matches('\\');
            let manager = match index.is_manager(class) {
                Some(manager) => manager,
                None => {
                    let info = loader(Fqn::new(class))?;
                    if !is_subtype_of(&info, DRUPAL_PLUGIN_MANAGER, loader) {
                        return None;
                    }
                    let mut candidates = index.types.iter().filter(|plugin_type| {
                        loader(Fqn::new(&plugin_type.manager))
                            .is_some_and(|manager| is_subtype_of(&manager, class, loader))
                    });
                    match (candidates.next(), candidates.next()) {
                        (Some(only), None) => only.manager,
                        (Some(_), Some(_)) => return None,
                        // A subclass of a manager that declares no type of
                        // its own has its parent's.
                        (None, _) => {
                            index
                                .types
                                .iter()
                                .find(|plugin_type| {
                                    is_subtype_of(&info, &plugin_type.manager, loader)
                                })?
                                .manager
                        }
                    }
                }
            };
            if found.is_some_and(|found| found != manager) {
                return None;
            }
            found = Some(manager);
        }
        found
    }

    /// The plugin classes the plugin ID `key` (see [`plugin_key`]) names.
    pub(crate) fn drupal_plugin_classes_named(&self, key: &str) -> Vec<Atom> {
        let (manager, id) = split_plugin_key(key);
        let mut classes: Vec<Atom> = self
            .drupal_plugin_index()
            .named(manager, id)
            .iter()
            .map(|plugin| plugin.class)
            .collect();
        classes.dedup();
        classes
    }

    /// Where the plugin ID `key` is declared: the name of each class that
    /// declares it, or its base plugin's for a derivative.
    pub(crate) fn drupal_plugin_definitions(&self, key: &str) -> Vec<Location> {
        self.drupal_plugin_classes_named(key)
            .iter()
            .filter_map(|class| self.class_declaration_location(class))
            .collect()
    }

    /// The hover detail of the plugin ID `key`: its manager and the classes
    /// that declare it.
    pub(crate) fn drupal_plugin_detail(&self, key: &str) -> String {
        let (manager, id) = split_plugin_key(key);
        let classes = self.drupal_plugin_classes_named(key);
        let mut detail = format!("Plugin manager `{manager}`");
        if classes.is_empty() {
            detail.push_str("\n\nNot declared by any plugin class in the project");
            return detail;
        }
        if !self
            .drupal_plugin_index()
            .plugins_of(manager)
            .iter()
            .any(|plugin| plugin.id == id)
        {
            detail.push_str("\n\nA derivative of the base plugin");
        }
        for class in classes {
            detail.push_str(&format!("\n\nClass `{class}`"));
        }
        detail
    }

    /// Every plugin ID of `manager`, sorted.
    pub(crate) fn drupal_plugin_ids(&self, manager: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .drupal_plugin_index()
            .plugins_of(manager)
            .iter()
            .map(|plugin| plugin.id.clone())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// The plugin classes of the manager a receiver of the class `class`
    /// is, for go-to-implementation on a manager or on an interface only
    /// one manager implements.
    pub(crate) fn drupal_plugin_implementations(&self, class: &str) -> Vec<Atom> {
        if !self.is_drupal_project() {
            return Vec::new();
        }
        let loader = self.resolved_class_loader_with(&[]);
        let Some(manager) = self.drupal_plugin_manager_of(&[class.to_string()], &loader) else {
            return Vec::new();
        };
        let mut classes: Vec<Atom> = self
            .drupal_plugin_index()
            .plugins_of(&manager)
            .iter()
            .map(|plugin| plugin.class)
            .collect();
        classes.dedup();
        classes
    }
}

/// The plugin classes `createInstance(id)` on `owner`, a plugin manager,
/// returns.  `None` for another call, or an ID the project declares no
/// plugin for, so the call keeps its declared type.
pub(crate) fn instance_classes(
    backend: &Backend,
    owner: &ClassInfo,
    method_name: &str,
    id: &str,
    class_loader: &ResolvedClassLoader<'_>,
) -> Option<Vec<Arc<ClassInfo>>> {
    if !method_name.eq_ignore_ascii_case("createInstance") || !backend.is_drupal_project() {
        return None;
    }
    let manager = backend.drupal_plugin_manager_of(&[owner.fqn().to_string()], class_loader)?;
    let classes: Vec<Arc<ClassInfo>> = backend
        .drupal_plugin_classes_named(&plugin_key(&manager, id))
        .iter()
        .filter_map(|class| class_loader(Fqn::new(class)))
        .collect();
    (!classes.is_empty()).then_some(classes)
}

impl Backend {
    /// Complete the widget or formatter the `type` of a field's display
    /// options names: `setDisplayOptions('form', ['type' => '|'])`.
    pub(crate) fn try_drupal_display_type_completion(
        &self,
        content: &str,
        position: tower_lsp_server::gen_lsp_types::Position,
        code: &crate::completion::source::code_context::CodeContext<'_>,
    ) -> Option<tower_lsp_server::gen_lsp_types::CompletionResponse> {
        let (quote, _) = code.open_string?;
        if crate::drupal::elements::arrow_key(code.code_before)? != "type" {
            return None;
        }
        let (_, paren) = code.nested_pair(b'[', b'(')?;
        let call = code
            .open_brackets
            .iter()
            .rev()
            .find(|bracket| bracket.offset == paren)?;
        if call.commas != 1 {
            return None;
        }
        let method = content.get(call.callee_name_start?..call.code_before)?;
        if !method.eq_ignore_ascii_case("setDisplayOptions") {
            return None;
        }
        let first = content.get(paren + 1..)?;
        let first = first.get(..first.find(',')?)?.trim();
        let display = first
            .strip_prefix(['\'', '"'])
            .and_then(|rest| rest.strip_suffix(['\'', '"']))?;
        let manager = display_manager(display)?;
        let cursor = crate::text_position::position_to_offset(content, position) as usize;
        self.symfony_names_response(
            crate::symbol_map::StringKeyKind::DrupalPlugin,
            self.drupal_plugin_ids(manager),
            content,
            quote + 1,
            cursor,
            position,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{derivative_bases, discovery_relative_name};

    #[test]
    fn a_derivative_reads_as_its_bases_longest_first() {
        assert_eq!(derivative_bases("a:b:c").collect::<Vec<_>>(), ["a:b", "a"]);
        assert_eq!(derivative_bases("a").count(), 0);
    }

    #[test]
    fn a_plugin_class_is_read_below_its_extension_namespace() {
        assert_eq!(
            discovery_relative_name("Drupal\\acme\\Plugin\\Block\\Hello").as_deref(),
            Some("plugin\\block\\hello")
        );
        assert_eq!(
            discovery_relative_name("Drupal\\Core\\Field\\Plugin\\Field\\FieldType\\StringItem")
                .as_deref(),
            Some("plugin\\field\\fieldtype\\stringitem")
        );
        assert_eq!(discovery_relative_name("App\\Plugin\\Block\\Hello"), None);
    }
}
