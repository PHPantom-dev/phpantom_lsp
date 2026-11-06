//! The entities Drupal upcasts route parameters to.
//!
//! Drupal's entity converter loads the entity a route parameter names
//! before the controller runs, for the parameters core's
//! `EntityResolverManager::setRouteOptions()` gives an `entity:<type>`
//! type: the ones the route's `options.parameters` declare; failing that,
//! the controller's parameters that are named after an entity type and
//! type-hinted with its class or a class or interface it extends
//! (`NodeInterface $node`); and when that finds none, the parameter named
//! after the entity type an `_entity_view` or `_entity_form` default names.
//! A parameter is only converted when the route gives it a value, by its
//! path or a default, and only by the entity converter when its definition
//! names no converter or tempstore of its own.
//!
//! The routes are looked up by the method they dispatch to, in an index
//! read once until a file that declares routes changes (see
//! [`Backend::note_drupal_routes`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::Backend;
use crate::atom::Atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::{PhpType, TypeKind};
use crate::symbol_map::{DrupalRouteSite, SymbolMap};

/// The project's routes, by the methods they dispatch to.
#[derive(Default)]
pub(crate) struct RouteIndex {
    /// Each route whose controller or title callback is the method,
    /// keyed by it as `Class::method`, lowercased.
    by_handler: HashMap<String, Vec<Arc<DrupalRouteSite>>>,
    /// The files the routes were read from.
    uris: HashSet<String>,
}

/// The class a native type hint names, the way Drupal's
/// `Reflection::getParameterClassName()` reads it: a single class, which
/// may be nullable, but not a union or a builtin type.
pub(crate) fn hinted_class(hint: &PhpType) -> Option<&str> {
    let named = match hint.kind() {
        TypeKind::Nullable(inner) => inner,
        TypeKind::Union(members) if members.len() == 2 && members.iter().any(PhpType::is_null) => {
            members.iter().find(|member| !member.is_null())?
        }
        _ => hint,
    };
    named.class_name().map(|name| name.trim_start_matches('\\'))
}

/// The entity type a declared parameter type converts to: the one an
/// `entity:<type>` or `entity_revision:<type>` names, written out.
fn entity_type_of_parameter_type(ty: &str) -> Option<&str> {
    ty.strip_prefix("entity:")
        .or_else(|| ty.strip_prefix("entity_revision:"))
        .filter(|entity_type| !entity_type.is_empty() && !entity_type.contains('{'))
}

impl Backend {
    /// The routes whose controller or title callback is `class::method`.
    pub(crate) fn drupal_handler_routes(
        &self,
        class: &str,
        method: &str,
    ) -> Vec<Arc<DrupalRouteSite>> {
        let index = self.drupal_route_index();
        if index.by_handler.is_empty() {
            return Vec::new();
        }
        let mut key = class.trim_start_matches('\\').to_ascii_lowercase();
        key.push_str("::");
        key.push_str(&method.to_ascii_lowercase());
        index.by_handler.get(&key).cloned().unwrap_or_default()
    }

    /// What each parameter of `class::method` receives that every route
    /// dispatching to the method, or naming it its title callback, upcasts:
    /// the parameter's name without its `$`, the entity class, and whether a
    /// route's default leaves it `NULL`.  Empty for a method no route
    /// reaches.
    pub(crate) fn drupal_upcast_parameters(
        &self,
        class: &str,
        method: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Vec<(String, Atom, bool)> {
        if !self.is_drupal_project() {
            return Vec::new();
        }
        let routes = self.drupal_handler_routes(class, method);
        let mut upcast: Option<Vec<(String, String)>> = None;
        for route in &routes {
            let found = self.drupal_route_upcasts(route, class_loader);
            upcast = Some(match upcast {
                None => found,
                Some(kept) => kept
                    .into_iter()
                    .filter(|parameter| found.contains(parameter))
                    .collect(),
            });
        }
        upcast
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(parameter, entity_type)| {
                let (class, _) = self.drupal_entity_types(&entity_type).into_iter().next()?;
                let nullable = routes
                    .iter()
                    .any(|route| route.defaults_to_null(&parameter));
                Some((parameter, class, nullable))
            })
            .collect()
    }

    /// The parameters `route` upcasts, each with the entity type it is
    /// converted to, by the rules of `EntityResolverManager` and
    /// `ParamConverterManager`: a parameter the route gives a value, by the
    /// path or a default, whose definition ends up with an `entity:` or
    /// `entity_revision:` type and no converter of its own.
    pub(crate) fn drupal_route_upcasts(
        &self,
        route: &DrupalRouteSite,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> Vec<(String, String)> {
        // A type the routing file gives a parameter stays, and a converter
        // or a tempstore it names converts it some other way.
        let takes_a_type = |name: &str| {
            route
                .declared(name)
                .is_none_or(|declared| declared.ty.is_none() && !declared.converted_otherwise)
        };
        let mut upcast: Vec<(String, String)> = route
            .parameters
            .iter()
            .filter(|declared| !declared.converted_otherwise)
            .filter_map(|declared| {
                entity_type_of_parameter_type(declared.ty.as_deref()?)
                    .map(|entity_type| (declared.name.clone(), entity_type.to_string()))
            })
            .collect();

        // A parameter named after an entity type and hinted with a class its
        // entity class is takes the type.
        let mut reflected = false;
        if let Some((class, method)) = route
            .controller
            .as_deref()
            .and_then(|controller| controller.split_once("::"))
            && let Some(controller) = class_loader(Fqn::new(class))
            && let Some(method) = controller.get_method_ci(method)
        {
            for parameter in method.parameters.iter() {
                let name = parameter.name.trim_start_matches('$');
                if parameter
                    .native_type_hint
                    .as_ref()
                    .and_then(hinted_class)
                    .is_some_and(|hint| self.drupal_entity_class_is(name, hint, class_loader))
                {
                    reflected = true;
                    if takes_a_type(name) {
                        upcast.push((name.to_string(), name.to_string()));
                    }
                }
            }
        }

        // Failing that, the entity an `_entity_view` or `_entity_form` names,
        // unless a parameter already has its `entity:` type.
        if !reflected
            && let Some(entity_type) = &route.entity_type
            && crate::drupal::routing::path_parameter_names(&route.path)
                .any(|name| name == entity_type.as_str())
        {
            let own_type = format!("entity:{entity_type}");
            if !route
                .parameters
                .iter()
                .any(|declared| declared.ty.as_deref() == Some(own_type.as_str()))
                && takes_a_type(entity_type)
                && !self.drupal_entity_types(entity_type).is_empty()
            {
                upcast.push((entity_type.clone(), entity_type.clone()));
            }
        }

        upcast.retain(|(name, _)| route.passes(name));
        upcast
    }

    /// Whether the entity class of the entity type `entity_type` is the
    /// class `hint`, or extends or implements it.  `false` for an entity
    /// type the project does not declare, or whose class is not found.
    pub(crate) fn drupal_entity_class_is(
        &self,
        entity_type: &str,
        hint: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> bool {
        self.drupal_entity_types(entity_type)
            .first()
            .and_then(|(class, _)| class_loader(Fqn::new(class)))
            .is_some_and(|class| crate::class_lookup::is_subtype_of(&class, hint, class_loader))
    }

    /// The project's routes by the methods they dispatch to, read once until
    /// a file that declares routes changes (see
    /// [`Backend::note_drupal_routes`]).
    fn drupal_route_index(&self) -> Arc<RouteIndex> {
        if let Some(index) = self.drupal.route_index.read().as_ref() {
            return Arc::clone(index);
        }
        let mut index = RouteIndex::default();
        self.visit_declaring_symbol_maps(|uri, map| {
            if !map.drupal_routes.is_empty() {
                index.uris.insert(uri.to_string());
            }
            for route in &map.drupal_routes {
                let route = Arc::new(route.clone());
                for handler in [&route.controller, &route.title_callback]
                    .into_iter()
                    .flatten()
                {
                    index
                        .by_handler
                        .entry(handler.to_ascii_lowercase())
                        .or_default()
                        .push(Arc::clone(&route));
                }
            }
        });
        let index = Arc::new(index);
        *self.drupal.route_index.write() = Some(Arc::clone(&index));
        index
    }

    /// Forget what [`Backend::drupal_route_index`] read when the routes of
    /// `uri` may have changed: `map`, the map just published for it, declares
    /// some, or the index read some from it.  `None` for a file that is gone.
    pub(crate) fn note_drupal_routes(&self, uri: &str, map: Option<&SymbolMap>) {
        let stale = match self.drupal.route_index.read().as_ref() {
            None => false,
            Some(index) => {
                map.is_some_and(|map| !map.drupal_routes.is_empty()) || index.uris.contains(uri)
            }
        };
        if stale {
            *self.drupal.route_index.write() = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hint_names_a_class_when_it_is_one_maybe_nullable() {
        let named = |name: &str| PhpType::named(crate::atom::atom(name));
        assert_eq!(
            hinted_class(&named("Drupal\\node\\NodeInterface")),
            Some("Drupal\\node\\NodeInterface")
        );
        assert_eq!(
            hinted_class(&PhpType::nullable(named("Drupal\\node\\Node"))),
            Some("Drupal\\node\\Node")
        );
        assert_eq!(hinted_class(&named("string")), None);
        assert_eq!(
            hinted_class(&PhpType::union(vec![
                named("Drupal\\node\\Node"),
                named("string")
            ])),
            None
        );
    }

    #[test]
    fn declared_types_convert_to_an_entity_type_written_out() {
        assert_eq!(entity_type_of_parameter_type("entity:node"), Some("node"));
        assert_eq!(
            entity_type_of_parameter_type("entity_revision:node"),
            Some("node")
        );
        assert_eq!(entity_type_of_parameter_type("entity:{entity_type}"), None);
        assert_eq!(entity_type_of_parameter_type("tempstore"), None);
    }
}
