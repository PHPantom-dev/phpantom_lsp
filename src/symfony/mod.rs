//! Symfony's service container, router, translator, Twig, and bundle
//! configuration: the service IDs, parameters, route names, translation
//! keys, templates, and configuration keys a project declares, and where.
//!
//! Every declaration and every use is a
//! [`StringKey`](crate::symbol_map::SymbolKind::StringKey) span in some
//! file's symbol map: YAML and XML configuration, translation catalogues,
//! and Twig templates through the resource symbol maps (see [`resources`],
//! [`routes`], [`translations`], [`crate::twig`], and [`config_tree`]), and
//! PHP through symbol-map extraction.  Template names are Twig's, so they
//! are read in any project that uses Twig, Symfony or not.
//!
//! Event names and Messenger bus names are string keys too (see [`tags`]
//! and [`messenger`]).  Form fields, validation mappings, and the listener
//! methods an event subscription names name a class's members, so they are
//! member accesses instead (see [`forms`] and [`validation`]).  The
//! Messenger handlers a project registers are recorded beside the spans of
//! the file that declares them (see [`messenger`]).  What a service or
//! parameter declaration gives its name, a class or a value, is recorded
//! beside its span too, for the type engine (see [`container`]).
//! A declaration is a span with `is_write` set, or a configurator call
//! whose receiver only the type engine can confirm (a
//! [`TypedReceiverSite`](crate::symbol_map::TypedReceiverSite) with
//! `is_write` set).  The few files holding one, or a Messenger handler, are
//! tracked here, so the declared names are read from those maps rather than
//! from a second index.
//!
//! Yii's route names are declared the same way, by spans in the PHP files
//! that define its routes (see [`crate::yii`]), so the files holding one, or
//! a route of any kind, are tracked here too.

pub(crate) mod bundles;
pub(crate) mod config_tree;
pub(crate) mod container;
pub(crate) mod forms;
pub(crate) mod messenger;
pub(crate) mod resources;
pub(crate) mod route_world;
pub(crate) mod routes;
pub(crate) mod tags;
pub(crate) mod translations;
pub(crate) mod validation;

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use parking_lot::RwLock;
use tower_lsp_server::gen_lsp_types::{Location, Range};

use crate::Backend;
use crate::symbol_map::{KeyFramework, StringKeyKind, SymbolKind, SymbolMap, SymbolSpan};
use crate::text_position::LineIndex;

/// The two formats Symfony's configuration is read from as text.
pub(crate) enum ConfigFormat {
    Yaml,
    Xml,
}

/// The format of the configuration file at `uri`, by its extension.
pub(crate) fn config_format(uri: &str) -> Option<ConfigFormat> {
    let path = uri
        .split(['?', '#'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase();
    let path = path.strip_suffix(".dist").unwrap_or(&path);
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        Some(ConfigFormat::Yaml)
    } else if path.ends_with(".xml") {
        Some(ConfigFormat::Xml)
    } else {
        None
    }
}

/// A place in a configuration file that holds a name: the content range of
/// the value, which is empty while the name is still being typed, so that
/// completion and the symbol map read the file the same way.
pub(crate) struct NameSlot {
    pub kind: StringKeyKind,
    pub start: usize,
    pub end: usize,
    pub is_write: bool,
}

/// The spans the `slots` of `content` that hold a name make.
pub(crate) fn slot_spans(content: &str, slots: Vec<NameSlot>) -> Vec<SymbolSpan> {
    slots
        .into_iter()
        .filter(|slot| slot.start < slot.end)
        .filter_map(|slot| {
            Some(SymbolSpan {
                start: slot.start as u32,
                end: slot.end as u32,
                kind: SymbolKind::StringKey {
                    kind: slot.kind,
                    key: content.get(slot.start..slot.end)?.to_string(),
                    is_write: slot.is_write,
                    is_optional: false,
                },
            })
        })
        .collect()
}

/// The files whose symbol maps declare a Symfony service ID, container
/// parameter, route, translation key, Twig template, Twig filter or
/// function, configuration key,
/// event listener, or Messenger bus, or register a Messenger handler, or
/// define a Yii route.
#[derive(Default)]
pub(crate) struct SymfonyState {
    declaring_uris: RwLock<HashSet<String>>,
    /// The routing files of dependencies, read from disk.
    foreign_routing: route_world::ForeignRoutingFiles,
    /// The service IDs and parameters the installed bundles declare, once
    /// read.
    /// Held while they are read, so the workers that ask at once read them
    /// once.
    bundle_declarations: parking_lot::Mutex<Option<std::sync::Arc<bundles::BundleDeclarations>>>,
    /// The translation domains an undeclared key is reported in.
    judged_translation_domains: translations::JudgedDomains,
}

/// Whether `map` declares a service ID, a container parameter, a route, a
/// translation key, a template, a Twig filter or function, a configuration
/// key, an event listener, a Messenger bus, a Drupal entity type, a Drupal
/// route, or a Drupal hook, outright or through a configurator call still
/// to be confirmed, or registers a Messenger handler, or imports routes, or
/// declares a Twig loader.
///
/// Only the kinds `enabled` takes count: outside Symfony, a project that
/// uses Twig declares its templates, filters, and functions and nothing
/// else.  A Yii route is
/// counted whether it has a name or not, for the arguments its handler
/// reads from its path.
fn declares_symfony_keys(map: &SymbolMap, enabled: impl Fn(StringKeyKind) -> bool) -> bool {
    let counts = |kind: StringKeyKind| match kind.framework() {
        KeyFramework::ServiceContainer
        | KeyFramework::Symfony
        | KeyFramework::Twig
        | KeyFramework::Yii
        | KeyFramework::Drupal => enabled(kind),
        KeyFramework::Laravel | KeyFramework::WordPress => false,
    };
    (!map.messenger_handlers.is_empty() && enabled(StringKeyKind::MessengerBus))
        || (!map.routes.is_empty() && enabled(StringKeyKind::YiiRoute))
        || (!map.route_imports.is_empty() && enabled(StringKeyKind::SymfonyRoute))
        || (map.declares_twig_loader && enabled(StringKeyKind::TwigTemplate))
        || ((!map.drupal_route_patterns.is_empty() || !map.drupal_route_aliases.is_empty())
            && enabled(StringKeyKind::DrupalRoute))
        || map.spans.iter().any(|span| {
            matches!(
                &span.kind,
                SymbolKind::StringKey {
                    kind,
                    is_write: true,
                    ..
                } if counts(*kind)
            )
        })
        || map
            .typed_receiver_sites
            .iter()
            .any(|site| site.is_write && site.receiver.names_any(counts))
}

impl Backend {
    /// Whether the project builds a Symfony service container; see
    /// [`Backend::is_symfony`](crate::Backend::is_symfony).
    pub(crate) fn is_symfony_project(&self) -> bool {
        self.is_symfony.load(Ordering::Relaxed)
    }

    /// Record whether the project builds a Symfony service container, once
    /// the workspace's `composer.json` files have been read.
    pub(crate) fn set_is_symfony(&self, is_symfony: bool) {
        self.is_symfony.store(is_symfony, Ordering::Relaxed);
    }

    /// Whether the framework a string key of `kind` belongs to is the one
    /// this project uses.
    pub(crate) fn string_key_kind_enabled(&self, kind: StringKeyKind) -> bool {
        match kind.framework() {
            KeyFramework::Laravel => self.resolved_class_cache.read().laravel().is_enabled(),
            KeyFramework::ServiceContainer => self.reads_service_container(),
            KeyFramework::Symfony => self.is_symfony_project(),
            KeyFramework::Twig => self.is_twig_project(),
            KeyFramework::Yii => self.is_yii_project(),
            KeyFramework::Drupal => self.is_drupal_project(),
            // The hook functions' names are WordPress's own, and a project
            // that calls them is a WordPress one whatever its
            // `composer.json` says, or whether it has one at all.
            KeyFramework::WordPress => true,
        }
    }

    /// Keep the set of declaring files in step with a published symbol map.
    ///
    /// Dependencies are never part of the project's own configuration, so a
    /// vendor file that happens to be parsed is left out.
    pub(crate) fn note_symfony_declarations(&self, uri: &str, map: &SymbolMap) {
        if !self.reads_service_container() && !self.is_twig_project() && !self.is_yii_project() {
            return;
        }
        let declares = declares_symfony_keys(map, |kind| self.string_key_kind_enabled(kind))
            && !self.is_closed_drupal_test_fixture(uri)
            && !self
                .workspace
                .vendor_uri_prefixes
                .lock()
                .iter()
                .any(|prefix| uri.starts_with(prefix.as_str()));
        self.note_drupal_routes(uri, Some(map));
        // Only take the write lock when the answer changes.
        if self.symfony.declaring_uris.read().contains(uri) == declares {
            return;
        }
        let mut uris = self.symfony.declaring_uris.write();
        if declares {
            uris.insert(uri.to_string());
        } else {
            uris.remove(uri);
        }
    }

    /// Forget a file that no longer exists.
    pub(crate) fn forget_symfony_declarations(&self, uri: &str) {
        self.note_drupal_routes(uri, None);
        if self.symfony.declaring_uris.read().contains(uri) {
            self.symfony.declaring_uris.write().remove(uri);
        }
    }

    /// Visit the symbol map of every file that declares a name, in a
    /// stable order.
    pub(crate) fn visit_declaring_symbol_maps(&self, mut visit: impl FnMut(&str, &SymbolMap)) {
        let mut uris: Vec<String> = self.symfony.declaring_uris.read().iter().cloned().collect();
        uris.sort_unstable();
        for uri in uris {
            if let Some(map) = self.symbol_map_for(&uri) {
                visit(&uri, &map);
            }
        }
    }

    /// Visit every declaration of a `kind` key: the file and its symbol
    /// map, the span's byte range, and the key.  A route is visited under
    /// each name the imports that load it give it (see [`route_world`]),
    /// and a service ID or parameter the installed bundles declare after
    /// the project's own (see [`bundles`]).
    pub(crate) fn visit_symfony_declarations(
        &self,
        kind: StringKeyKind,
        mut visit: impl FnMut(&str, &SymbolMap, u32, u32, &str),
    ) {
        self.visit_project_symfony_declarations(kind, &mut visit);
        self.visit_bundle_declarations(kind, visit);
    }

    /// [`Self::visit_symfony_declarations`], leaving out what the installed
    /// bundles declare.
    fn visit_project_symfony_declarations(
        &self,
        kind: StringKeyKind,
        mut visit: impl FnMut(&str, &SymbolMap, u32, u32, &str),
    ) {
        // The configurator calls are confirmed in their own files, whose
        // offsets have nothing to do with the file a diagnostic pass may be
        // checking on this thread, so neither of its caches may answer for
        // them.
        let _isolated = (
            crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
            crate::type_engine::resolver::with_isolated_chain_cache(),
        );
        if kind == StringKeyKind::SymfonyRoute {
            let world = self.symfony_route_world();
            self.visit_route_declarations(&world, visit);
            return;
        }
        let mut uris: Vec<String> = self.symfony.declaring_uris.read().iter().cloned().collect();
        uris.sort_unstable();
        // Only Symfony declares names through calls the type engine has to
        // confirm, and Drupal its hooks, which the calls that invoke them
        // declare; Drupal's other names never are.
        let needs_typed = match kind.framework() {
            KeyFramework::Drupal => kind == StringKeyKind::DrupalHook,
            KeyFramework::ServiceContainer => self.is_symfony_project(),
            KeyFramework::Laravel
            | KeyFramework::Symfony
            | KeyFramework::Twig
            | KeyFramework::Yii
            | KeyFramework::WordPress => true,
        };
        for uri in uris {
            let Some(map) = self.symbol_map_for(&uri) else {
                continue;
            };
            let typed = if needs_typed {
                self.typed_receiver_spans_for(&uri, &map)
            } else {
                std::sync::Arc::default()
            };
            for span in map.spans.iter().chain(typed.iter()) {
                if let SymbolKind::StringKey {
                    kind: span_kind,
                    key,
                    is_write: true,
                    ..
                } = &span.kind
                    && *span_kind == kind
                {
                    visit(&uri, &map, span.start, span.end, key);
                }
            }
        }
    }

    /// Every service ID, container parameter, route name, translation key
    /// (with its domain, see [`translations::translation_key`]),
    /// translation domain, or template the project declares, sorted.
    pub(crate) fn symfony_declared_names(&self, kind: StringKeyKind) -> Vec<String> {
        self.symfony_declared_names_except(kind, None)
    }

    /// Every route name the project declares, sorted, with the world they
    /// were read from, which tells which of the names nothing declares
    /// are mistakes.
    pub(crate) fn symfony_routes(&self) -> (Vec<String>, route_world::RouteWorld) {
        // See `visit_symfony_declarations`.
        let _isolated = (
            crate::type_engine::variable::forward_walk::suspend_diagnostic_scope(),
            crate::type_engine::resolver::with_isolated_chain_cache(),
        );
        let world = self.symfony_route_world();
        let mut names = Vec::new();
        self.visit_route_declarations(&world, |_, _, _, _, name| names.push(name.to_string()));
        names.sort_unstable();
        names.dedup();
        (names, world)
    }

    /// Whether the project declares a Twig loader of its own, which can
    /// serve template names no file under the Twig roots holds.
    pub(crate) fn declares_twig_loader(&self) -> bool {
        let mut declares = false;
        self.visit_declaring_symbol_maps(|_, map| declares |= map.declares_twig_loader);
        declares
    }

    /// The service IDs or parameters the project's own configuration
    /// declares, sorted, leaving out the installed bundles'.
    pub(crate) fn symfony_project_declared_names(&self, kind: StringKeyKind) -> Vec<String> {
        let mut names = Vec::new();
        self.visit_project_symfony_declarations(kind, |_, _, _, _, key| {
            names.push(key.to_string());
        });
        names.sort_unstable();
        names.dedup();
        names
    }

    /// [`Self::symfony_declared_names`], leaving out the declaration that
    /// starts at `except` (a file and an offset): the one being typed,
    /// which an event listener's subscription is.
    pub(crate) fn symfony_declared_names_except(
        &self,
        kind: StringKeyKind,
        except: Option<(&str, u32)>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        if kind == StringKeyKind::TranslationDomain {
            self.visit_symfony_declarations(
                StringKeyKind::SymfonyTranslation,
                |_, _, _, _, key| {
                    let (domain, _) = translations::split_translation_key(key);
                    if names.last().is_none_or(|last: &String| last != domain) {
                        names.push(domain.to_string());
                    }
                },
            );
        } else {
            // A parameter set under a name built at runtime is declared as a
            // pattern (`app.*`), which is no name anyone writes, and a route
            // is named after its controller (`App\Controller\Foo::bar`) as
            // well as by its own name, which is the one to offer.
            let is_unlisted = |key: &str| match kind {
                StringKeyKind::ContainerParameter => key.ends_with('*'),
                StringKeyKind::SymfonyRoute => key.contains('\\'),
                _ => false,
            };
            self.visit_symfony_declarations(kind, |uri, _, start, _, key| {
                if except != Some((uri, start)) && !is_unlisted(key) {
                    names.push(key.to_string());
                }
            });
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The messages the project declares in the translation domain
    /// `domain`, sorted.
    pub(crate) fn symfony_translation_messages(&self, domain: &str) -> Vec<String> {
        let mut messages = Vec::new();
        self.visit_symfony_declarations(StringKeyKind::SymfonyTranslation, |_, _, _, _, key| {
            let (declared, message) = translations::split_translation_key(key);
            if declared == domain {
                messages.push(message.to_string());
            }
        });
        messages.sort_unstable();
        messages.dedup();
        messages
    }

    /// The catalogues that declare the translation domain `domain`, each
    /// located at its start.
    fn symfony_domain_locations(&self, domain: &str) -> Vec<Location> {
        let mut uris: Vec<String> = self
            .symfony
            .declaring_uris
            .read()
            .iter()
            .filter(|uri| translations::catalogue_domain(uri) == Some(domain))
            .cloned()
            .collect();
        uris.sort_unstable();
        uris.iter()
            .filter_map(|uri| crate::util::parse_url(uri))
            .map(|uri| Location {
                uri,
                range: Range::default(),
            })
            .collect()
    }

    /// Where the project declares the service ID, container parameter,
    /// route name, translation key, translation domain, or template `key`.
    ///
    /// A service declared under its own class name is that class, and its
    /// ID is spelled as one: one with no declaration of its own (the usual
    /// autowired service) resolves to the class.  A translation domain is
    /// declared by the catalogues named after it.
    pub(crate) fn symfony_declaration_locations(
        &self,
        kind: StringKeyKind,
        key: &str,
    ) -> Vec<Location> {
        if kind == StringKeyKind::TranslationDomain {
            return self.symfony_domain_locations(key);
        }
        let mut found: Vec<(String, u32, u32)> = Vec::new();
        self.visit_symfony_declarations(kind, |uri, _, start, end, declared| {
            if declared == key {
                found.push((uri.to_string(), start, end));
            }
        });

        // Visited file by file, so each file's text is read once.
        let mut locations = Vec::with_capacity(found.len());
        for group in found.chunk_by(|a, b| a.0 == b.0) {
            let uri = &group[0].0;
            let (Some(parsed), Some(content)) =
                (crate::util::parse_url(uri), self.get_file_content_arc(uri))
            else {
                continue;
            };
            let lines = LineIndex::new(&content);
            for (_, start, end) in group {
                locations.push(Location {
                    uri: parsed.clone(),
                    range: Range {
                        start: lines.position(*start as usize),
                        end: lines.position(*end as usize),
                    },
                });
            }
        }

        if locations.is_empty()
            && kind == StringKeyKind::ServiceId
            && key.contains('\\')
            && let Some(location) = self.class_declaration_location(key)
        {
            locations.push(location);
        }
        locations
    }
}

#[cfg(test)]
mod tests;
