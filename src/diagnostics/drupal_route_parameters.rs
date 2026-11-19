//! Drupal route parameters whose type hint rejects the raw ID Drupal
//! passes.
//!
//! Drupal loads the entity a parameter stands for only when the route
//! upcasts it (see [`crate::drupal::upcasting`]).  A controller parameter
//! named after an entity type and hinted with a class unrelated to its
//! entity class is not upcast, so it is passed the raw ID, which the hint
//! rejects:
//!
//! ```php
//! // node.view: path: '/node/{node}'
//! public function view(UserInterface $node): array // receives '42'
//! ```
//!
//! The warning gives a verdict only where it is sure, and stays silent
//! otherwise: on title callbacks, on controllers PHPantom cannot reflect
//! the way core does, on parameters the route's `options` declare or an
//! `_entity_*` default may upcast, and on hints whose class hierarchy it
//! cannot load.  A parameter is reported only when every route that gives
//! it a value, by its path or a default, passes it the raw ID.

use tower_lsp_server::gen_lsp_types::*;

use crate::Backend;
use crate::atom::{Atom, AtomSet};
use crate::class_lookup::{Fqn, ResolvedClassLoader, is_subtype_of};
use crate::symbol_map::{DrupalRouteSite, SymbolKind};
use crate::text_position::offset_to_position;
use crate::types::ClassInfo;

use super::helpers::{FileDiagnosticContext, make_diagnostic};

/// Diagnostic code used for a route parameter whose hint blocks upcasting.
pub(crate) const INVALID_DRUPAL_ROUTE_PARAMETER_CODE: &str = "invalid_drupal_route_parameter";

impl Backend {
    /// Report each parameter of a route's controller whose class hint
    /// rejects the raw ID every route that reaches the method passes it.
    pub(crate) fn collect_drupal_route_parameter_diagnostics(
        &self,
        ctx: &FileDiagnosticContext,
        uri: &str,
        content: &str,
        out: &mut Vec<Diagnostic>,
    ) {
        if !self.is_drupal_project() || self.is_drupal_example_or_test_code(uri) {
            return;
        }
        let class_loader = self.resolved_class_loader(&ctx.file);
        for class in &ctx.file.classes {
            let fqn = class.fqn();
            for method in class.methods.iter() {
                let routes = self.drupal_handler_routes(&fqn, &method.name);
                if routes.is_empty() {
                    continue;
                }
                let handler = format!("{}::{}", fqn.trim_start_matches('\\'), method.name);
                for parameter in method.parameters.iter() {
                    let name = parameter.name.trim_start_matches('$');
                    let Some(hint) = parameter
                        .native_type_hint
                        .as_ref()
                        .and_then(crate::drupal::upcasting::hinted_class)
                    else {
                        continue;
                    };
                    let mut passing = routes.iter().filter(|route| route.passes(name)).peekable();
                    if passing.peek().is_none()
                        || !passing.all(|route| {
                            self.passes_rejected_id(route, &handler, name, hint, &class_loader)
                        })
                    {
                        continue;
                    }
                    let Some((start, end)) = parameter_span(ctx, method.name_offset, name) else {
                        continue;
                    };
                    out.push(make_diagnostic(
                        Range {
                            start: offset_to_position(content, start as usize),
                            end: offset_to_position(content, end as usize),
                        },
                        DiagnosticSeverity::Warning,
                        INVALID_DRUPAL_ROUTE_PARAMETER_CODE,
                        format!(
                            "Drupal passes ${name} the raw ID, which its type {} rejects, because no route to this method upcasts it",
                            hint.trim_start_matches('\\')
                        ),
                    ));
                }
            }
        }
    }

    /// Whether `route`, which passes the parameter `name` of `handler`
    /// (`Class::method`), surely passes it the raw ID its class hint `hint`
    /// rejects.  `false` whenever that is not certain.
    fn passes_rejected_id(
        &self,
        route: &DrupalRouteSite,
        handler: &str,
        name: &str,
        hint: &str,
        class_loader: &ResolvedClassLoader<'_>,
    ) -> bool {
        // Core reflects only on the method the route dispatches to, which
        // `controller` names only when it is a class's.
        if !route
            .controller
            .as_deref()
            .is_some_and(|controller| controller.eq_ignore_ascii_case(handler))
        {
            return false;
        }
        // A declared parameter is converted as declared, and an `_entity_*`
        // default may upcast it.
        if route.declared(name).is_some() || route.entity_type.is_some() {
            return false;
        }
        // A `NULL` default the path does not override is passed as it is.
        if route.defaults_to_null(name)
            && !crate::drupal::routing::path_parameter_names(&route.path)
                .any(|placeholder| placeholder == name)
        {
            return false;
        }
        let Some(hinted) =
            class_loader(Fqn::new(hint)).filter(|hinted| hierarchy_loads(hinted, class_loader))
        else {
            return false;
        };
        let entities = self.drupal_entity_types(name);
        !entities.is_empty()
            && entities.iter().all(|(entity, _)| {
                class_loader(Fqn::new(entity))
                    .filter(|class| hierarchy_loads(class, class_loader))
                    // A subclass is a bundle class, which core's reflection
                    // does not upcast to, but `hook_entity_type_alter()` can
                    // make it the entity class.
                    .is_some_and(|class| {
                        !is_subtype_of(&class, hint, class_loader)
                            && !is_subtype_of(&hinted, entity, class_loader)
                    })
            })
    }
}

/// Whether every class and interface `class` extends or implements loads,
/// so that subtyping against it is decided rather than cut short.  A
/// keyword type is no class to load: PHP's stubs declare `\Traversable`
/// to extend `iterable`, which every content entity reaches.
fn hierarchy_loads(class: &ClassInfo, class_loader: &ResolvedClassLoader<'_>) -> bool {
    let mut pending: Vec<Atom> = class
        .parent_class
        .into_iter()
        .chain(class.interfaces.iter().copied())
        .collect();
    let mut seen = AtomSet::default();
    while let Some(name) = pending.pop() {
        if !seen.insert(name) || !crate::php_type::is_class_like_name(name.trim_start_matches('\\'))
        {
            continue;
        }
        let Some(ancestor) = class_loader(Fqn::new(&name)) else {
            return false;
        };
        pending.extend(
            ancestor
                .parent_class
                .into_iter()
                .chain(ancestor.interfaces.iter().copied()),
        );
    }
    true
}

/// The byte range of the parameter `name` of the method whose name starts
/// at `name_offset`: the first `$name` after it.
fn parameter_span(ctx: &FileDiagnosticContext, name_offset: u32, name: &str) -> Option<(u32, u32)> {
    let spans = &ctx.symbol_map.spans;
    let from = spans.partition_point(|span| span.start <= name_offset);
    spans[from..].iter().find_map(|span| match &span.kind {
        SymbolKind::Variable { name: variable } if variable.as_str() == name => {
            Some((span.start, span.end))
        }
        _ => None,
    })
}
