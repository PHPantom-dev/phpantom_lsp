use std::path::Path;

use crate::symbol_map::StringKeyKind;

/// A Drupal project's backend whose web root is `web/` under the
/// workspace, with `files` published the way the workspace index publishes
/// them.
fn drupal_backend(files: &[(&str, &str)]) -> (crate::Backend, tempfile::TempDir) {
    backend_with_web_root(files, true)
}

/// A backend with a web root `web/` under the workspace, marked as a
/// Drupal project only when `is_drupal` holds.
fn backend_with_web_root(
    files: &[(&str, &str)],
    is_drupal: bool,
) -> (crate::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.set_is_symfony(false);
    if is_drupal {
        backend.mark_drupal_project();
    }
    backend.note_drupal_web_root(&dir.path().join("web"));
    for (path, content) in files {
        let uri = uri(dir.path(), path);
        backend.update_resource_symbol_index(&uri, content);
    }
    (backend, dir)
}

fn uri(root: &Path, path: &str) -> String {
    crate::util::path_to_uri(&root.join(path))
}

const SERVICES: &str = "parameters:\n  acme.limit: 10\nservices:\n  acme.greeter:\n    class: Drupal\\acme\\Greeter\n    arguments: ['@config.factory']\n";

#[test]
fn services_files_in_the_web_root_declare_services_and_parameters() {
    let (backend, _dir) = drupal_backend(&[
        ("web/modules/custom/acme/acme.services.yml", SERVICES),
        (
            "web/core/core.services.yml",
            "services:\n  config.factory:\n    class: Drupal\\Core\\Config\\ConfigFactory\n",
        ),
        (
            "web/sites/default/services.yml",
            "parameters:\n  session.storage.options: {}\n",
        ),
    ]);
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::ServiceId),
        vec!["acme.greeter".to_string(), "config.factory".to_string()]
    );
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::ContainerParameter),
        vec![
            "acme.limit".to_string(),
            "session.storage.options".to_string()
        ]
    );
    let classes: Vec<String> = backend
        .symfony_service_classes("acme.greeter")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(classes, vec!["Drupal\\acme\\Greeter".to_string()]);
}

/// Default configuration gives names by its file names alone, so editing
/// one keeps the configuration index; a schema edit, and a test module's
/// file opening or closing, drop it.
#[test]
fn only_what_changes_the_names_drops_the_configuration_index() {
    let (backend, dir) = drupal_backend(&[]);
    let default = dir
        .path()
        .join("web/modules/custom/acme/config/install/acme.settings.yml");
    let schema = "web/modules/custom/acme/config/schema/acme.schema.yml";
    let fixture =
        "web/core/modules/system/tests/modules/config_test/config/install/config_test.system.yml";
    backend.note_drupal_config_files(vec![default.clone()], vec![dir.path().join(schema)]);
    let first = backend.drupal_config_index();

    backend.note_drupal_config_document(&crate::util::path_to_uri(&default), Some(""));
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &backend.drupal_config_index()
    ));

    backend.note_drupal_config_document(&uri(dir.path(), schema), Some("acme.settings: {}\n"));
    let second = backend.drupal_config_index();
    assert!(!std::sync::Arc::ptr_eq(&first, &second));

    let fixture_uri = uri(dir.path(), fixture);
    backend.open_files.write().insert(
        fixture_uri.clone(),
        std::sync::Arc::new("a: b\n".to_string()),
    );
    backend.note_drupal_config_document(&fixture_uri, Some("a: b\n"));
    let opened = backend.drupal_config_index();
    assert!(!std::sync::Arc::ptr_eq(&second, &opened));
    assert!(opened.names().contains(&"config_test.system".to_string()));
    // Typing in it changes no name.
    backend.note_drupal_config_document(&fixture_uri, Some("a: b\n"));
    assert!(std::sync::Arc::ptr_eq(
        &opened,
        &backend.drupal_config_index()
    ));

    backend.open_files.write().remove(&fixture_uri);
    backend.note_drupal_config_document(&fixture_uri, Some("a: b\n"));
    assert!(
        !backend
            .drupal_config_index()
            .names()
            .contains(&"config_test.system".to_string())
    );
}

#[test]
fn a_routing_file_under_a_config_directory_is_drupals_in_a_symfony_site() {
    // Core's `config` module puts a `config` segment on the way to its
    // routing file, which a Symfony site reads as container configuration.
    let (backend, dir) = drupal_backend(&[]);
    backend.set_is_symfony(true);
    backend.update_resource_symbol_index(
        &uri(dir.path(), "web/core/modules/config/config.routing.yml"),
        "config.sync:\n  path: '/admin/config/development/configuration'\n",
    );
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::DrupalRoute),
        vec!["config.sync".to_string()]
    );
}

/// The route names `content`, published as the template at `path`, reads,
/// with the kind of route each is read as.
fn template_routes(
    backend: &crate::Backend,
    root: &Path,
    path: &str,
    content: &str,
) -> Vec<(StringKeyKind, String)> {
    let template = uri(root, path);
    backend.update_resource_symbol_index(&template, content);
    let map = backend
        .symbol_map_for(&template)
        .expect("the template's map");
    map.spans
        .iter()
        .filter_map(|span| match &span.kind {
            crate::symbol_map::SymbolKind::StringKey { kind, key, .. }
                if matches!(
                    kind,
                    StringKeyKind::DrupalRoute | StringKeyKind::SymfonyRoute
                ) =>
            {
                Some((*kind, key.clone()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_drupal_sites_templates_name_drupal_routes_in_a_symfony_project() {
    let (backend, dir) = drupal_backend(&[]);
    backend.set_is_symfony(true);
    assert_eq!(
        template_routes(
            &backend,
            dir.path(),
            "web/modules/custom/acme/templates/acme.html.twig",
            "{{ path('acme.greeting') }}\n",
        ),
        vec![(StringKeyKind::DrupalRoute, "acme.greeting".to_string())]
    );
}

#[test]
fn a_template_outside_the_web_root_names_symfony_routes() {
    // A Symfony application beside the Drupal site, in the same workspace.
    let (backend, dir) = drupal_backend(&[]);
    backend.set_is_symfony(true);
    assert_eq!(
        template_routes(
            &backend,
            dir.path(),
            "app/templates/base.html.twig",
            "{{ path('blog_index') }}\n",
        ),
        vec![(StringKeyKind::SymfonyRoute, "blog_index".to_string())]
    );
}

#[test]
fn other_yaml_is_not_read_as_services() {
    let (backend, _dir) = drupal_backend(&[
        // A file with a `services:` key that Drupal does not load services from.
        (
            "web/modules/custom/acme/acme.info.yml",
            "services:\n  not.a_service: ~\n",
        ),
        ("docker-compose.yml", "services:\n  php: ~\n"),
        // A services file outside every web root.
        ("tools/acme.services.yml", "services:\n  tools.thing: ~\n"),
        // Templates and copies Drupal never loads as they stand.
        (
            "web/sites/default/default.services.yml",
            "services:\n  template.thing: ~\n",
        ),
        (
            "web/core/assets/scaffold/files/development.services.yml",
            "services:\n  scaffold.thing: ~\n",
        ),
        (
            "web/sites/simpletest/123/services.yml",
            "services:\n  test.thing: ~\n",
        ),
    ]);
    assert!(
        backend
            .symfony_declared_names(StringKeyKind::ServiceId)
            .is_empty()
    );
}

#[test]
fn drupal_reads_the_service_container_and_nothing_else_from_symfony() {
    let (backend, _dir) = drupal_backend(&[]);
    for kind in [
        StringKeyKind::ServiceId,
        StringKeyKind::ContainerParameter,
        StringKeyKind::SymfonyEvent,
    ] {
        assert!(backend.string_key_kind_enabled(kind), "{kind:?}");
    }
    for kind in [
        StringKeyKind::SymfonyRoute,
        StringKeyKind::SymfonyTranslation,
        StringKeyKind::MessengerBus,
        StringKeyKind::SymfonyConfigKey,
    ] {
        assert!(!backend.string_key_kind_enabled(kind), "{kind:?}");
    }
}

#[test]
fn a_symfony_config_directory_means_nothing_in_drupal() {
    let (backend, _dir) =
        drupal_backend(&[("config/services.yaml", "services:\n  app.mailer: ~\n")]);
    assert!(
        backend
            .symfony_declared_names(StringKeyKind::ServiceId)
            .is_empty()
    );
}

#[test]
fn nothing_is_read_outside_a_drupal_project() {
    let (backend, dir) = backend_with_web_root(&[], false);
    backend.update_resource_symbol_index(
        &uri(dir.path(), "web/modules/custom/acme/acme.services.yml"),
        SERVICES,
    );
    assert!(
        backend
            .symfony_declared_names(StringKeyKind::ServiceId)
            .is_empty()
    );
}

#[test]
fn a_test_modules_services_are_read_only_while_it_is_open() {
    let path = "web/core/modules/system/tests/modules/fixture/fixture.services.yml";
    let content = "services:\n  fixture.thing: ~\n";
    let (backend, dir) = drupal_backend(&[(path, content)]);
    assert!(
        backend
            .symfony_declared_names(StringKeyKind::ServiceId)
            .is_empty()
    );

    let uri = uri(dir.path(), path);
    backend
        .open_files
        .write()
        .insert(uri.clone(), std::sync::Arc::new(content.to_string()));
    backend.update_resource_symbol_index(&uri, content);
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::ServiceId),
        vec!["fixture.thing".to_string()]
    );
}

#[test]
fn core_and_contrib_are_read_only_and_custom_code_is_not() {
    let (backend, dir) = drupal_backend(&[]);
    for (path, read_only) in [
        ("web/core/lib/Drupal.php", true),
        ("web/modules/contrib/token/token.services.yml", true),
        ("web/themes/contrib/gin/gin.theme", true),
        ("web/profiles/contrib/demo/demo.profile", true),
        ("web/libraries/chart/chart.php", true),
        ("web/modules/custom/acme/acme.services.yml", false),
        ("web/themes/custom/acme_theme/acme_theme.theme", false),
        ("web/sites/default/services.yml", false),
        ("tools/core/script.php", false),
    ] {
        assert_eq!(
            backend.is_drupal_read_only_uri(&uri(dir.path(), path)),
            read_only,
            "{path}"
        );
    }
}

#[test]
fn moves_and_namespace_renames_leave_contrib_alone() {
    let (backend, dir) = drupal_backend(&[]);
    let contrib = uri(dir.path(), "web/modules/contrib/token/src/Token.php");
    backend.update_ast(
        &contrib,
        "<?php\nnamespace Drupal\\token;\nclass Token {}\n",
    );
    let custom = uri(dir.path(), "web/modules/custom/acme/src/Acme.php");
    backend.update_ast(&custom, "<?php\nnamespace Drupal\\acme;\nclass Acme {}\n");

    let namespace = backend.plan_namespace_move("Drupal\\token", "Drupal\\tok");
    assert!(
        namespace
            .as_ref()
            .is_err_and(|message| message.contains("installed")),
        "{namespace:?}"
    );
    let class = backend.plan_class_move("Drupal\\token\\Token", "Drupal\\tok\\Token");
    assert!(
        class
            .as_ref()
            .is_err_and(|message| message.contains("installed")),
        "{class:?}"
    );
    assert!(
        backend
            .plan_namespace_move("Drupal\\acme", "Drupal\\acme2")
            .is_ok()
    );
}

/// Outside a Drupal site, a namespace a vendor package shares is renamed on
/// the project's side, and the vendor files are left as they are.
#[test]
fn a_namespace_a_vendor_package_shares_is_renamed_outside_drupal() {
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    *backend.workspace.vendor_uri_prefixes.lock() = vec![uri(dir.path(), "vendor/")];
    for (path, content) in [
        (
            "vendor/acme/lib/src/Shared/Thing.php",
            "<?php\nnamespace App\\Shared;\nclass Thing {}\n",
        ),
        (
            "src/Shared/Mine.php",
            "<?php\nnamespace App\\Shared;\nclass Mine {}\n",
        ),
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
        backend.update_ast(&uri(dir.path(), path), content);
    }

    let plan = backend.plan_namespace_move("App\\Shared", "App\\Common");
    let edit = format!("{:?}", plan.as_ref().expect("the rename is planned"));
    assert!(edit.contains("src/Shared/Mine.php"), "{edit}");
    assert!(!edit.contains("vendor/acme"), "{edit}");
}

#[test]
fn route_patterns_match_any_run_for_each_star() {
    use super::matches_route_pattern;
    assert!(matches_route_pattern(
        "search.view_*",
        "search.view_node_search"
    ));
    assert!(matches_route_pattern(
        "*.bc",
        "entity.block_content.edit_form.bc"
    ));
    assert!(matches_route_pattern(
        "layout_builder.*.view",
        "layout_builder.overrides.node.view"
    ));
    assert!(matches_route_pattern("acme.*", "acme."));
    assert!(!matches_route_pattern("search.view_*", "search.help_x"));
    assert!(!matches_route_pattern(
        "layout_builder.*.view",
        "layout_builder.overrides.node.edit"
    ));
    assert!(!matches_route_pattern("a*a", "a"));
    assert!(matches_route_pattern("exact", "exact"));
    assert!(!matches_route_pattern("exact", "exactly"));
}

/// The key of the hash a Drupal template's `path()` passes is a parameter
/// of the route, never a Twig variable, also in a project that reads
/// Twig's variables; the value after it stays a variable.
#[test]
fn a_path_hash_key_is_a_route_parameter_not_a_variable() {
    let (backend, dir) = drupal_backend(&[]);
    backend.set_is_twig(true);
    let content = "{{ path('entity.node.canonical', {node: node.id}) }}\n";
    let template = uri(
        dir.path(),
        "web/modules/custom/acme/templates/acme.html.twig",
    );
    backend.update_resource_symbol_index(&template, content);
    let map = backend
        .symbol_map_for(&template)
        .expect("the template's map");
    let kinds_at = |offset: usize| -> Vec<(StringKeyKind, String)> {
        map.spans
            .iter()
            .filter(|span| span.start as usize == offset)
            .filter_map(|span| match &span.kind {
                crate::symbol_map::SymbolKind::StringKey { kind, key, .. } => {
                    Some((*kind, key.clone()))
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        kinds_at(content.find("node:").unwrap()),
        [(
            StringKeyKind::DrupalRouteParameter,
            "entity.node.canonical#node".to_string()
        )]
    );
    assert_eq!(
        kinds_at(content.find("node.id").unwrap()),
        [(StringKeyKind::TwigVariable, "node".to_string())]
    );
}
