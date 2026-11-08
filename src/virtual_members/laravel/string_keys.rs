//! Go-to-definition and find-references for string-key spans.
//!
//! Laravel encodes several kinds of navigable references as plain string
//! literals: `config('app.name')`, `view('emails.welcome')`,
//! `route('users.index')`, `__('messages.saved')`, and so does Symfony's
//! container (`$container->get('app.mailer')`).  The symbol map records
//! these as [`crate::symbol_map::StringKey`] spans; this module turns
//! a span (kind + key) into concrete definition/reference [`Location`]s.

use super::{
    find_all_config_references, resolve_config_key_declaration,
    resolve_config_key_declaration_exact,
};
use super::{route_names, trans_keys, view_names};

use tower_lsp_server::gen_lsp_types::Location;

/// Unified go-to-definition entry point for all Laravel string-key spans.
///
/// Dispatches on [`crate::symbol_map::StringKeyKind`] so callers in
/// `definition/resolve.rs` only need one import and one call site.  Adding a
/// new Laravel navigation feature only requires a new match arm here, not a
/// new `pub(crate) use` in the parent module.
///
/// `uri` is the file the key was written in.  Most kinds name something the
/// project holds exactly one of and ignore it; a Blade section or stack name
/// only means anything relative to the template that wrote it, since its
/// other half is in whatever renders that template.
pub(crate) fn resolve_laravel_string_key(
    backend: &crate::Backend,
    kind: &crate::symbol_map::StringKeyKind,
    key: &str,
    uri: &str,
) -> Vec<Location> {
    use crate::symbol_map::StringKeyKind;
    match kind {
        StringKeyKind::TempestView { relative } => backend
            .tempest_view_definition(uri, key, *relative)
            .into_iter()
            .collect(),
        StringKeyKind::Section => {
            backend.blade_block_definitions(uri, crate::blade::blocks::BlockKind::Section, key)
        }
        StringKeyKind::Stack => {
            backend.blade_block_definitions(uri, crate::blade::blocks::BlockKind::Stack, key)
        }
        StringKeyKind::Config => {
            if crate::symbol_map::laravel_resources::resource_from_config_key(key).is_some() {
                resolve_config_key_declaration_exact(backend, key)
                    .into_iter()
                    .collect()
            } else {
                resolve_config_key_declaration(backend, key)
                    .into_iter()
                    .collect()
            }
        }
        StringKeyKind::ConfigResource(resource) => {
            let config_key = crate::symbol_map::laravel_resources::config_key(*resource, key);
            resolve_config_key_declaration_exact(backend, &config_key)
                .into_iter()
                .collect()
        }
        StringKeyKind::View => view_names::resolve_view_definitions(backend, key),
        StringKeyKind::Route => route_names::resolve_route_definitions(backend, key),
        StringKeyKind::Trans => trans_keys::resolve_trans_definitions(backend, key),
        StringKeyKind::Command => resolve_command_definition(backend, key)
            .into_iter()
            .collect(),
        StringKeyKind::MorphAlias => resolve_morph_alias_definitions(backend, key),
        StringKeyKind::GateAbility => resolve_gate_ability_definitions(backend, key),
        StringKeyKind::Middleware => resolve_middleware_definitions(backend, key),
        StringKeyKind::ContainerBinding => resolve_container_binding_definitions(backend, key),
        StringKeyKind::Env => super::env_vars::resolve_env_definitions(backend, key),
        StringKeyKind::Asset
        | StringKeyKind::MixAsset
        | StringKeyKind::ViteAsset
        | StringKeyKind::InertiaPage => super::resolve_asset_definitions(backend, *kind, key),
        StringKeyKind::ServiceId
        | StringKeyKind::ContainerParameter
        | StringKeyKind::SymfonyRoute
        | StringKeyKind::SymfonyTranslation
        | StringKeyKind::TranslationDomain
        | StringKeyKind::TwigTemplate
        | StringKeyKind::SymfonyConfigKey
        | StringKeyKind::SymfonyEvent
        | StringKeyKind::MessengerBus
        | StringKeyKind::SerializerGroup
        | StringKeyKind::YiiRoute
        | StringKeyKind::DrupalEntityType
        | StringKeyKind::DrupalRoute
        | StringKeyKind::DrupalRouteParameter
        | StringKeyKind::DrupalPermission
        | StringKeyKind::DrupalLibrary => backend.symfony_declaration_locations(*kind, key),
        StringKeyKind::DrupalHook => backend.drupal_hook_documentation(key),
        StringKeyKind::DrupalConfigName => backend.drupal_config_locations(key),
        StringKeyKind::DrupalConfigKey => backend.drupal_config_key_locations(key),
        StringKeyKind::DrupalRenderElement => backend.drupal_render_element_definitions(key),
        StringKeyKind::DrupalThemeHook => backend.drupal_theme_hook_definitions(key),
        StringKeyKind::DrupalThemeHookVariable => backend.drupal_theme_variable_definitions(key),
        StringKeyKind::YiiView => backend.resolve_yii_view_definitions(key),
        StringKeyKind::YiiAlias => backend.yii_alias_definitions(key),
        StringKeyKind::YiiParam => backend.yii_param_definitions(key),
        StringKeyKind::TwigBlock => backend.twig_block_definitions(uri, None, key),
        StringKeyKind::TwigVariable => backend.twig_variable_definitions(uri, None, key),
        StringKeyKind::TwigFilter | StringKeyKind::TwigFunction => {
            backend.twig_callable_definitions(*kind, key)
        }
        StringKeyKind::YiiRouteArgument => backend.yii_route_argument_locations(key),
        StringKeyKind::YiiDefinitionArgument => backend.yii_definition_argument_locations(key),
        StringKeyKind::WordPressHook => backend.wordpress_hook_definitions(key),
    }
}

/// Resolve a container binding key to the service-provider registration that
/// bound it and to the class it resolves to.
///
/// The registration comes first, matching every other string kind: a config
/// key jumps to the config file, a morph alias to its `morphMap()` entry.  A
/// core alias the framework declares has no registration of its own, so the
/// bound class is the only answer it has.
fn resolve_container_binding_definitions(backend: &crate::Backend, key: &str) -> Vec<Location> {
    let Some(target) = backend.container_binding_target(key) else {
        return Vec::new();
    };

    let mut locations = Vec::new();
    if let Some(site) = target.site
        && let Some(parsed_uri) = crate::util::parse_url(&site.uri)
        && let Some(content) = backend.get_file_content(&site.uri)
    {
        let position = crate::text_position::offset_to_position(&content, site.offset as usize);
        locations.push(crate::definition::point_location(parsed_uri, position));
    }
    if let Some(location) = backend.class_declaration_location(&target.fqn) {
        locations.push(location);
    }
    locations
}

/// Resolve an authorization ability to every place it is declared: the
/// `Gate::define()` call that registers it, and the policy methods that
/// implement it.
///
/// A gate definition comes first — it applies to any model, so it is the
/// broadest answer — followed by each policy method, ordered by policy FQN so
/// the list is stable between requests.
fn resolve_gate_ability_definitions(backend: &crate::Backend, ability: &str) -> Vec<Location> {
    let mut locations = Vec::new();

    let definition = backend
        .laravel
        .gates
        .read()
        .definition(ability)
        .map(|target| (target.uri.clone(), target.offset));
    if let Some((uri, offset)) = definition
        && let Some(parsed_uri) = crate::util::parse_url(&uri)
        && let Some(content) = backend.get_file_content(&uri)
    {
        let position = crate::text_position::offset_to_position(&content, offset as usize);
        locations.push(crate::definition::point_location(parsed_uri, position));
    }

    for (policy, method) in super::gates::policy_methods_named(backend, ability) {
        if let Some(location) = policy_method_location(backend, &policy, &method)
            && !locations
                .iter()
                .any(|l| l.uri == location.uri && l.range.start == location.range.start)
        {
            locations.push(location);
        }
    }

    locations
}

/// The [`Location`] of a policy method's name token.
///
/// `policy` is the class that *declares* the method, so its `name_offset`
/// indexes that class's own file.  A synthesized member — an `@method` tag —
/// has no offset at all, and there the policy's declaration is the closest
/// thing to a location the ability has.
fn policy_method_location(
    backend: &crate::Backend,
    policy: &crate::types::ClassInfo,
    method: &crate::types::MethodInfo,
) -> Option<Location> {
    let fqn = policy.fqn();
    let at_declaration = (method.name_offset != 0)
        .then(|| {
            let uri = backend
                .symbols
                .fqn_uri_index
                .read()
                .get(fqn.as_str())
                .cloned()?;
            let content = backend.get_file_content(&uri)?;
            let position =
                crate::text_position::offset_to_position(&content, method.name_offset as usize);
            Some(crate::definition::point_location(
                crate::util::parse_url(&uri)?,
                position,
            ))
        })
        .flatten();

    at_declaration.or_else(|| backend.class_declaration_location(&fqn))
}

/// Resolve a morph alias to its `Relation::morphMap()` registration and to the
/// model it maps to.
///
/// The registration comes first, matching every other string kind (a config key
/// jumps to the config file, a view name to the template).  The mapped model
/// follows so that go-to-definition also offers the class the alias stands for.
fn resolve_morph_alias_definitions(backend: &crate::Backend, alias: &str) -> Vec<Location> {
    let Some((fqn, uri, offset)) = backend
        .laravel
        .morph_map
        .read()
        .get(alias)
        .map(|target| (target.fqn.clone(), target.uri.clone(), target.offset))
    else {
        return Vec::new();
    };

    let mut locations = Vec::new();
    if let Some(parsed_uri) = crate::util::parse_url(&uri)
        && let Some(content) = backend.get_file_content(&uri)
    {
        let position = crate::text_position::offset_to_position(&content, offset as usize);
        locations.push(crate::definition::point_location(parsed_uri, position));
    }
    if let Some(location) = backend.class_declaration_location(&fqn) {
        locations.push(location);
    }
    locations
}

/// Resolve a middleware alias or group to where it is registered, and an
/// alias to the middleware class it runs as well.
fn resolve_middleware_definitions(backend: &crate::Backend, name: &str) -> Vec<Location> {
    let (site, class) = {
        let index = backend.laravel.middleware.read();
        match index.get(name) {
            Some(super::MiddlewareTarget::Alias(alias)) => {
                ((alias.uri.clone(), alias.offset), alias.class.clone())
            }
            Some(super::MiddlewareTarget::Group(group)) => {
                ((group.uri.clone(), group.offset), None)
            }
            None => return Vec::new(),
        }
    };

    let mut locations = Vec::new();
    let (uri, offset) = site;
    if let Some(parsed_uri) = crate::util::parse_url(&uri)
        && let Some(content) = backend.get_file_content(&uri)
    {
        let position = crate::text_position::offset_to_position(&content, offset as usize);
        locations.push(crate::definition::point_location(parsed_uri, position));
    }
    if let Some(class) = class
        && let Some(location) = backend.class_declaration_location(&class)
    {
        locations.push(location);
    }
    locations
}

/// Resolve an Artisan command name to the declaration site inside its
/// command class (the `$signature` / `$name` / `#[AsCommand]` literal).
fn resolve_command_definition(backend: &crate::Backend, name: &str) -> Option<Location> {
    let index = backend.laravel.commands.read();
    let entry = index.get(name)?;
    let uri = crate::util::parse_url(&entry.uri)?;
    let content = backend.get_file_content(&entry.uri)?;
    let position = crate::text_position::offset_to_position(&content, entry.name_offset as usize);
    Some(crate::definition::point_location(uri, position))
}

/// Unified find-references entry point for all Laravel string-key spans.
///
/// Dispatches on [`crate::symbol_map::StringKeyKind`] — see
/// [`resolve_laravel_string_key`] for the same rationale.
pub(crate) fn find_laravel_string_key_references(
    backend: &crate::Backend,
    kind: &crate::symbol_map::StringKeyKind,
    key: &str,
    uri: &str,
    snapshot: &[(String, std::sync::Arc<crate::symbol_map::SymbolMap>)],
    include_declaration: bool,
) -> Vec<Location> {
    use crate::symbol_map::StringKeyKind;
    let mut locations = match kind {
        StringKeyKind::Config | StringKeyKind::ConfigResource(_) => {
            find_all_config_references(backend, kind, key, snapshot, include_declaration)
        }
        // Two unrelated pages that both fill `content` fill two different
        // sections, so the span index's project-wide answer is the wrong
        // one: only the templates that render each other share a name.
        StringKeyKind::Section => {
            return backend.blade_block_references(
                uri,
                crate::blade::blocks::BlockKind::Section,
                key,
            );
        }
        StringKeyKind::Stack => {
            return backend.blade_block_references(
                uri,
                crate::blade::blocks::BlockKind::Stack,
                key,
            );
        }
        StringKeyKind::TempestView { relative } => {
            return backend.tempest_view_references(
                uri,
                key,
                *relative,
                snapshot,
                include_declaration,
            );
        }
        StringKeyKind::View
        | StringKeyKind::Route
        | StringKeyKind::Trans
        | StringKeyKind::Command
        | StringKeyKind::MorphAlias
        | StringKeyKind::GateAbility
        | StringKeyKind::Middleware
        | StringKeyKind::ContainerBinding
        | StringKeyKind::Env
        | StringKeyKind::Asset
        | StringKeyKind::MixAsset
        | StringKeyKind::ViteAsset
        | StringKeyKind::InertiaPage => find_string_key_usages(kind, key, backend, snapshot, true),
        // A Symfony declaration is a span in a configuration file's (or a
        // template's) map like any use, so the usages already hold it.
        StringKeyKind::ServiceId
        | StringKeyKind::ContainerParameter
        | StringKeyKind::SymfonyTranslation
        | StringKeyKind::TwigTemplate
        | StringKeyKind::TwigFilter
        | StringKeyKind::TwigFunction
        | StringKeyKind::SymfonyConfigKey
        | StringKeyKind::SymfonyEvent
        | StringKeyKind::MessengerBus
        | StringKeyKind::SerializerGroup
        | StringKeyKind::YiiRoute
        | StringKeyKind::DrupalEntityType
        | StringKeyKind::DrupalRoute
        | StringKeyKind::DrupalRouteParameter
        | StringKeyKind::DrupalPermission
        | StringKeyKind::DrupalRenderElement
        | StringKeyKind::DrupalLibrary => {
            return find_string_key_usages(kind, key, backend, snapshot, include_declaration);
        }
        // A documented pattern stands for every hook that fills it in.
        StringKeyKind::DrupalHook => {
            return backend.drupal_hook_references(key, include_declaration);
        }
        // The catalogues named after a domain declare it, and the paths of
        // its handler's routes a route argument, and are added below.  A
        // route's name may be put together by the imports that load it, or
        // declared by a bundle's routing file outside the workspace, so its
        // declarations are too, as are a theme hook's and its variables',
        // which `hook_theme()` arrays declare.
        StringKeyKind::TranslationDomain
        | StringKeyKind::YiiRouteArgument
        | StringKeyKind::YiiDefinitionArgument
        | StringKeyKind::YiiParam
        | StringKeyKind::SymfonyRoute
        | StringKeyKind::DrupalConfigName
        | StringKeyKind::DrupalConfigKey
        | StringKeyKind::DrupalThemeHook
        | StringKeyKind::DrupalThemeHookVariable => {
            find_string_key_usages(kind, key, backend, snapshot, false)
        }
        // The string that declares an alias is a span like any use, but
        // only the configuration says it declares one.
        StringKeyKind::YiiAlias => {
            let declared = backend.yii_alias_definitions(key);
            let mut locations = find_string_key_usages(kind, key, backend, snapshot, true);
            locations.retain(|location| !declared.contains(location));
            if include_declaration {
                locations.extend(declared);
            }
            return locations;
        }
        // Two spellings of one view share no key, so the renders are found
        // by the file they name rather than in the key's snapshot.
        StringKeyKind::YiiView => {
            return backend.find_yii_view_references(key, include_declaration);
        }
        // Like a Blade section, a block is shared only by the templates
        // that extend each other.
        StringKeyKind::TwigBlock => {
            return backend.twig_block_references(uri, None, key);
        }
        // A template's variable is its own, but a global is every
        // template's.
        StringKeyKind::TwigVariable if backend.is_twig_template_uri(uri) => {
            return backend.twig_variable_references(uri, None, key);
        }
        StringKeyKind::TwigVariable => {
            return find_string_key_usages(kind, key, backend, snapshot, include_declaration);
        }
        // A pattern only names a hook by matching it, so the places that
        // fire or use it are found by matching too.
        StringKeyKind::WordPressHook => {
            return backend.wordpress_hook_references(key, include_declaration, false);
        }
    };

    if include_declaration && !kind.is_config_backed() {
        for decl in resolve_laravel_string_key(backend, kind, key, uri) {
            // A declaration is in its file's own coordinates, as
            // go-to-definition reports it, but every other location here
            // indexes a template's virtual PHP, and the `references` handler
            // translates the whole list back as though it all did.
            let decl_uri = decl.uri.as_str();
            let range = if backend.is_blade_file(decl_uri) {
                backend.translate_template_range_to_php(decl_uri, decl.range)
            } else {
                decl.range
            };
            crate::references::push_location(&mut locations, &decl.uri, range.start, range.end);
        }
    }

    locations
}

/// Scan pre-built [`crate::symbol_map::SymbolMap`] spans for all call sites
/// matching `kind` + `key` — zero file re-parses, O(total spans) memory walk.
///
/// `include_writes` set to `false` leaves out the spans that declare the key.
fn find_string_key_usages(
    kind: &crate::symbol_map::StringKeyKind,
    key: &str,
    backend: &crate::Backend,
    snapshot: &[(String, std::sync::Arc<crate::symbol_map::SymbolMap>)],
    include_writes: bool,
) -> Vec<Location> {
    use crate::references::push_location;
    use crate::symbol_map::SymbolKind;
    use crate::text_position::LineIndex;

    let mut locations = Vec::new();
    for (file_uri, symbol_map) in snapshot {
        // A call whose receiver only a type settles is not in the map, so
        // ask for the file's confirmed extras — but only when a candidate
        // names this very key, so the type resolution is paid for the
        // handful of files that could contribute a hit.
        let has_candidate = symbol_map
            .typed_receiver_sites
            .iter()
            .any(|site| site.may_name(*kind, key));
        let extra = if has_candidate {
            backend.typed_receiver_spans_for(file_uri, symbol_map)
        } else {
            std::sync::Arc::new(Vec::new())
        };
        let morph_columns = (*kind == crate::symbol_map::StringKeyKind::MorphAlias
            && symbol_map
                .morph_column_sites
                .iter()
                .any(|site| site.key == key))
        .then(|| backend.morph_column_spans_for(file_uri, symbol_map));

        // First pass: check if this file even has ANY StringKey matches.
        // This avoids reading file content from disk for thousands of unrelated files.
        let spans = || {
            symbol_map
                .spans
                .iter()
                .chain(extra.iter())
                .chain(morph_columns.iter().flat_map(|spans| spans.iter()))
        };
        let matches = |span: &crate::symbol_map::SymbolSpan| {
            matches!(
                &span.kind,
                SymbolKind::StringKey {
                    kind: span_kind,
                    key: span_key,
                    is_write,
                    ..
                } if span_kind == kind && span_key == key && (include_writes || !is_write)
            )
        };
        let has_match = spans().any(matches);

        if !has_match {
            continue;
        }

        let Some(parsed_uri) = crate::util::parse_url(file_uri) else {
            continue;
        };
        // A template's spans index the virtual PHP it lowers to, which the
        // `references` handler translates back into the template afterwards.
        let Some(content) = backend.reference_file_content_arc(file_uri) else {
            continue;
        };
        let lines = LineIndex::new(&content);
        for span in spans().filter(|span| matches(span)) {
            let start = lines.position(span.start as usize);
            let end = lines.position(span.end as usize);
            push_location(&mut locations, &parsed_uri, start, end);
        }
    }
    locations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_map::{LaravelConfigResource, StringKeyKind};

    #[test]
    fn configured_resources_resolve_exact_entries_without_file_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config/cache.php");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            "<?php return ['stores' => ['redis' => ['driver' => 'redis']]];\n",
        )
        .unwrap();

        let backend = crate::Backend::new_test();
        *backend.workspace.workspace_root.write() = Some(dir.path().to_path_buf());
        let usage_uri = "file:///project/usage.php";
        let resource_kind = StringKeyKind::ConfigResource(LaravelConfigResource::CacheStore);

        let short = resolve_laravel_string_key(&backend, &resource_kind, "redis", usage_uri);
        let full = resolve_laravel_string_key(
            &backend,
            &StringKeyKind::Config,
            "cache.stores.redis",
            usage_uri,
        );
        assert_eq!(short, full);
        assert_eq!(short.len(), 1);

        assert!(
            resolve_laravel_string_key(&backend, &resource_kind, "missing", usage_uri).is_empty()
        );
        assert!(
            resolve_laravel_string_key(
                &backend,
                &StringKeyKind::Config,
                "cache.stores.missing",
                usage_uri,
            )
            .is_empty()
        );

        let generic = resolve_laravel_string_key(
            &backend,
            &StringKeyKind::Config,
            "cache.unlisted",
            usage_uri,
        );
        assert_eq!(generic.len(), 1);
        assert_eq!(
            generic[0].range.start,
            tower_lsp_server::gen_lsp_types::Position::new(0, 0)
        );
    }
}
