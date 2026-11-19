//! Configured components and discovery roots participate in the same catalogue.

use crate::common::{
    create_psr4_workspace, definition_locations, goto_definition_at, open_php, position_of,
    workspace_uri,
};
use tower_lsp_server::{LanguageServer, gen_lsp_types::*};

#[tokio::test]
async fn tempest_discovery_configuration_registers_paths_and_named_components() {
    let config = r#"<?php
use Tempest\View\ViewConfig as Views;
use Tempest\View\ViewComponent as Component;
const CARD = __DIR__ . '/../ui/x-card.view.php';
$paths = [CARD, __DIR__ . '/../ui/x-alert.view.php'];
$config = new Views();
$config->addViewComponents(...$paths);
$config->addViewComponent(Component::fromPath(__DIR__ . '/../ui/x-footer.view.php'));
$panel = new Component(name: 'x-renamed', contents: '', file: __DIR__ . '/../ui/panel.view.php', isVendorComponent: false);
$components = [$panel];
$config->addViewComponents(...$components);
return $config;
"#;
    let source = "<x-card /><x-alert /><x-footer /><x-renamed />";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/views.config.php", config),
            ("app/page.view.php", source),
            ("ui/x-card.view.php", "Card"),
            ("ui/x-alert.view.php", "Alert"),
            ("ui/x-footer.view.php", "Footer"),
            ("ui/panel.view.php", "Panel"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    for (name, path) in [
        ("x-card", "ui/x-card.view.php"),
        ("x-alert", "ui/x-alert.view.php"),
        ("x-footer", "ui/x-footer.view.php"),
        ("x-renamed", "ui/panel.view.php"),
    ] {
        let position = position_of(source, name);
        let locations = definition_locations(
            goto_definition_at(&backend, &uri, position.line, position.character + 2).await,
        );
        let expected =
            Uri::from_file_path(dunce::canonicalize(dir.path().join(path)).unwrap()).unwrap();
        assert_eq!(
            locations
                .iter()
                .map(|location| &location.uri)
                .collect::<Vec<_>>(),
            vec![&expected],
            "{name}"
        );
    }
}

#[tokio::test]
async fn tempest_discovery_extra_locations_respect_ignored_paths() {
    let config = r#"<?php
use Tempest\Discovery\DiscoveryConfig;
use Tempest\Discovery\DiscoveryLocation as Location;
$directory = dirname(__DIR__) . '/resources';
return new DiscoveryConfig(locations: [new Location('App\\', $directory, ignore: [$directory . '/private'])]);
"#;
    let source = "<x-public /><x-private />";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/discovery.config.php", config),
            ("app/page.view.php", source),
            ("resources/x-public.view.php", "Public"),
            ("resources/private/x-private.view.php", "Private"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    open_php(
        &backend,
        &workspace_uri(&backend, "resources/private/x-private.view.php"),
        "Changed private component",
    )
    .await;
    let position = position_of(source, "x-public");
    let locations = definition_locations(
        goto_definition_at(&backend, &uri, position.line, position.character + 2).await,
    );
    assert_eq!(
        locations[0].uri,
        Uri::from_file_path(
            dunce::canonicalize(dir.path().join("resources/x-public.view.php")).unwrap()
        )
        .unwrap()
    );
    let position = position_of(source, "x-private");
    assert!(
        definition_locations(
            goto_definition_at(&backend, &uri, position.line, position.character + 2).await
        )
        .is_empty()
    );
}

#[tokio::test]
async fn tempest_discovery_edits_closes_and_watched_changes_refresh_the_catalogue() {
    let original = "<?php $config = new \\Tempest\\View\\ViewConfig(); $config->addViewComponent(__DIR__.'/../one/x-card.view.php'); return $config;";
    let changed = original.replace("/one/", "/two/");
    let source = "<x-card />";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/views.config.php", original),
            ("app/controller.php", "<?php"),
            ("app/page.view.php", source),
            ("one/x-card.view.php", "One"),
            ("two/x-card.view.php", "Two"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    let config_uri = workspace_uri(&backend, "app/views.config.php");
    open_php(&backend, &uri, source).await;
    let expected =
        |path| Uri::from_file_path(dunce::canonicalize(dir.path().join(path)).unwrap()).unwrap();
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, 0, 3).await)[0].uri,
        expected("one/x-card.view.php")
    );
    open_php(&backend, &config_uri, &changed).await;
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, 0, 3).await)[0].uri,
        expected("two/x-card.view.php")
    );
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: config_uri.clone(),
            },
        })
        .await;
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, 0, 3).await)[0].uri,
        expected("one/x-card.view.php")
    );
    std::fs::write(dir.path().join("app/views.config.php"), changed).unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: config_uri.clone(),
                kind: FileChangeType::Changed,
            }],
        })
        .await;
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, 0, 3).await)[0].uri,
        expected("two/x-card.view.php")
    );
    std::fs::remove_file(dir.path().join("app/views.config.php")).unwrap();
    // Another edit can refresh inference before the deletion event arrives.
    // The cached catalogue still names the now-missing configuration file.
    open_php(
        &backend,
        &workspace_uri(&backend, "app/controller.php"),
        "<?php // Configuration was removed on disk.",
    )
    .await;
    assert!(definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).is_empty());
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: config_uri,
                kind: FileChangeType::Deleted,
            }],
        })
        .await;
    assert!(definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).is_empty());
}

#[tokio::test]
async fn tempest_discovery_nested_configuration_and_skipped_subtrees() {
    let config = r#"<?php
$discovery = new \Tempest\Discovery\DiscoveryConfig([
    new \Tempest\Discovery\DiscoveryLocation('App\\', __DIR__.'/../resources'),
]);
$discovery->skipPaths(__DIR__.'/../resources/x-hidden.view.php', __DIR__.'/../resources/sub');
return $discovery;
"#;
    let nested = r#"<?php
use Tempest\View\{ViewConfig, ViewComponent};
return new ViewConfig(viewComponents: [
    'x-registered' => new ViewComponent('x-internal', '', __DIR__.'/../ui/plain.view.php', false),
]);
"#;
    let source = "<x-registered /><x-hidden /><x-child />";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/discovery.config.php", config),
            ("resources/views.config.php", nested),
            ("resources/x-hidden.view.php", "Hidden"),
            ("resources/sub/x-child.view.php", "Child"),
            ("ui/plain.view.php", "Registered"),
            ("app/page.view.php", source),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    for (name, expected) in [
        ("x-registered", Some("ui/plain.view.php")),
        ("x-hidden", None),
        ("x-child", None),
    ] {
        let pos = position_of(source, name);
        let locations = definition_locations(
            goto_definition_at(&backend, &uri, pos.line, pos.character + 2).await,
        );
        let expected: Vec<_> = expected
            .map(|path| {
                Uri::from_file_path(dunce::canonicalize(dir.path().join(path)).unwrap()).unwrap()
            })
            .into_iter()
            .collect();
        assert_eq!(
            locations
                .iter()
                .map(|loc| loc.uri.clone())
                .collect::<Vec<_>>(),
            expected,
            "{name}"
        );
    }
    let config_uri = workspace_uri(&backend, "app/discovery.config.php");
    open_php(
        &backend,
        &config_uri,
        "<?php return new \\Tempest\\Discovery\\DiscoveryConfig();",
    )
    .await;
    assert!(definition_locations(goto_definition_at(&backend, &uri, 0, 4).await).is_empty());
}

#[tokio::test]
async fn tempest_discovery_only_scans_opted_in_packages() {
    let source = "<x-opted /><x-ignored /><x-unrelated /><x-core /><x-dependent />";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/page.view.php", source),
            (
                "vendor/composer/installed.json",
                r#"{"packages":[
            {"name":"acme/opted","autoload":{"psr-4":{"Opted\\":"src/"}},"install-path":"../acme/opted","extra":{"tempest":{"can-discover":true,"ignore":["src/private"]}}},
            {"name":"acme/unrelated","autoload":{"psr-4":{"Unrelated\\":"src/"}},"install-path":"../acme/unrelated"},
            {"name":"tempest/core","autoload":{"psr-4":{"Tempest\\":"src/"}},"install-path":"../tempest/core"},
            {"name":"acme/dependent","autoload":{"psr-4":{"Dependent\\":"src/"}},"install-path":"../acme/dependent","require":{"tempest/view":"^3.0"}}
          ]}"#,
            ),
            (
                "vendor/acme/opted/composer.json",
                r#"{"autoload":{"psr-4":{"Opted\\":"src/"}}}"#,
            ),
            ("vendor/acme/opted/src/x-opted.view.php", "Opted"),
            (
                "vendor/acme/opted/src/private/x-ignored.view.php",
                "Ignored",
            ),
            (
                "vendor/acme/unrelated/composer.json",
                r#"{"autoload":{"psr-4":{"Unrelated\\":"src/"}}}"#,
            ),
            (
                "vendor/acme/unrelated/src/x-unrelated.view.php",
                "Unrelated",
            ),
            (
                "vendor/tempest/core/composer.json",
                r#"{"autoload":{"psr-4":{"Tempest\\":"src/"}}}"#,
            ),
            ("vendor/tempest/core/src/x-core.view.php", "Core"),
            (
                "vendor/acme/dependent/composer.json",
                r#"{"autoload":{"psr-4":{"Dependent\\":"src/"}}}"#,
            ),
            (
                "vendor/acme/dependent/src/x-dependent.view.php",
                "Dependent",
            ),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    for (name, path) in [
        ("x-opted", Some("vendor/acme/opted/src/x-opted.view.php")),
        ("x-core", Some("vendor/tempest/core/src/x-core.view.php")),
        (
            "x-dependent",
            Some("vendor/acme/dependent/src/x-dependent.view.php"),
        ),
        ("x-ignored", None),
        ("x-unrelated", None),
    ] {
        let pos = position_of(source, name);
        let locations = definition_locations(
            goto_definition_at(&backend, &uri, pos.line, pos.character + 2).await,
        );
        let expected: Vec<_> = path
            .map(|path| {
                Uri::from_file_path(dunce::canonicalize(dir.path().join(path)).unwrap()).unwrap()
            })
            .into_iter()
            .collect();
        assert_eq!(
            locations
                .iter()
                .map(|loc| loc.uri.clone())
                .collect::<Vec<_>>(),
            expected,
            "{name}"
        );
    }
}

#[tokio::test]
async fn tempest_discovery_new_configuration_refreshes_a_warm_catalogue() {
    let source = "<x-card />";
    let config = "<?php $config = new \\Tempest\\View\\ViewConfig(); $config->addViewComponent(__DIR__.'/../ui/x-card.view.php'); return $config;";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/page.view.php", source),
            ("ui/x-card.view.php", "Card"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    let config_uri = workspace_uri(&backend, "app/new.config.php");
    open_php(&backend, &uri, source).await;
    assert!(definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).is_empty());
    open_php(&backend, &config_uri, config).await;
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).len(),
        1
    );
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: config_uri.clone(),
            },
        })
        .await;
    assert!(definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).is_empty());
    std::fs::write(dir.path().join("app/new.config.php"), config).unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: config_uri,
                kind: FileChangeType::Created,
            }],
        })
        .await;
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).len(),
        1
    );
}

#[tokio::test]
async fn tempest_discovery_completes_registered_tag_names_without_an_x_prefix() {
    use crate::common::{completion_response, response_items};
    let config = "<?php return new \\Tempest\\View\\ViewConfig(viewComponents: ['notice' => new \\Tempest\\View\\ViewComponent('internal', '', __DIR__.'/../ui/panel.view.php', false)]);";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/page.view.php", ""),
            ("app/views.config.php", config),
            ("ui/panel.view.php", "Notice"),
            ("app/x-card.view.php", "Card"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    for (source, cursor, start, end, expected) in [
        ("<not", 4, 1, 4, Some("notice")),
        ("</not>", 5, 2, 5, Some("notice")),
        ("<notice />", 4, 1, 7, Some("notice")),
        ("<x-ca", 5, 1, 5, Some("x-card")),
        ("<div>", 4, 1, 4, None),
    ] {
        open_php(&backend, &uri, source).await;
        let items =
            response_items(completion_response(&backend, &uri, 0, cursor, None).await).unwrap();
        if let Some(expected) = expected {
            assert_eq!(items.len(), 1, "{source}: {items:?}");
            assert_eq!(items[0].label, expected);
            assert_eq!(
                items[0].text_edit,
                Some(CompletionItemTextEdit::TextEdit(TextEdit {
                    range: Range::new(Position::new(0, start), Position::new(0, end)),
                    new_text: expected.to_string(),
                })),
            );
        } else {
            assert!(items.is_empty(), "{source}: {items:?}");
        }
    }
}

#[tokio::test]
async fn tempest_discovery_registered_tag_attributes_follow_unsaved_content() {
    use crate::common::{completion_response, response_labels};
    let source = "<notice  />";
    let config = "<?php return new \\Tempest\\View\\ViewConfig(viewComponents: ['notice' => new \\Tempest\\View\\ViewComponent('internal', '', __DIR__.'/../ui/panel.view.php', false)]);";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("app/page.view.php", source),
            ("app/views.config.php", config),
            ("ui/panel.view.php", "<?php /** @var string $old */ ?>"),
        ],
    );
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    assert!(
        response_labels(completion_response(&backend, &uri, 0, 8, None).await)
            .contains(&"old".to_string())
    );
    open_php(
        &backend,
        &workspace_uri(&backend, "ui/panel.view.php"),
        "<?php /** @var string $message */ ?>",
    )
    .await;
    let names = response_labels(completion_response(&backend, &uri, 0, 8, None).await);
    assert!(names.contains(&"message".to_string()), "{names:?}");
    assert!(!names.contains(&"old".to_string()), "{names:?}");
    assert_eq!(
        definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).len(),
        1
    );
}

#[tokio::test]
async fn tempest_discovery_bootstrap_and_indexed_registrations_tolerate_incomplete_configuration() {
    let bootstrap = r"<?php
use Tempest\View\{ViewConfig, ViewComponent};
use Tempest\Discovery\DiscoveryLocation;
new DiscoveryLocation;
new DiscoveryLocation('App', $unknown);
new $dynamic;
$config = new ViewConfig();
$config->$method();
$config->unrelated();
$config->addViewComponents(...[...['card' => __DIR__.'/ui/x-card.view.php'], 'empty' => '', 'unknown' => $unknown]);
new ViewConfig(viewComponents: [
    new ViewComponent('x-unkeyed', '', __DIR__.'/ui/x-unkeyed.view.php', false),
    $unknown => new ViewComponent('x-unknown', '', __DIR__.'/ui/x-unknown.view.php', false),
]);
new ViewConfig(viewComponents: $unknown);
return $config;
";
    let setup = r"<?php namespace App;
$config = new \Tempest\View\ViewConfig();
$config->addViewComponent(__DIR__.'/../ui/x-indexed.view.php');
class OtherConfig { public function addViewComponent(string $path): self { return $this; } }
$other = new OtherConfig();
$other->addViewComponent(__DIR__.'/../ui/x-unrelated.view.php');
";
    let source = "<x-card /><x-indexed /><x-unkeyed /><x-unknown /><x-unrelated />";
    let (backend, _dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[
            ("tempest", bootstrap),
            (
                "vendor/acme/no-autoload/README.md",
                "Package without an autoloader",
            ),
            ("app/setup.php", setup),
            ("app/page.view.php", source),
            ("ui/x-card.view.php", "Card"),
            ("ui/x-indexed.view.php", "Indexed"),
            ("ui/x-unkeyed.view.php", "Unkeyed"),
            ("ui/x-unknown.view.php", "Unknown"),
            ("ui/x-unrelated.view.php", "Unrelated"),
            (
                "vendor/composer/installed.json",
                r#"{"packages":[{"extra":{"tempest":{"can-discover":true}}},{"name":"acme/no-autoload","extra":{"tempest":{"can-discover":true}}}]}"#,
            ),
        ],
    );
    open_php(&backend, &workspace_uri(&backend, "app/setup.php"), setup).await;
    let uri = workspace_uri(&backend, "app/page.view.php");
    open_php(&backend, &uri, source).await;
    for (name, expected) in [
        ("x-card", 1),
        ("x-indexed", 1),
        ("x-unkeyed", 0),
        ("x-unknown", 0),
        ("x-unrelated", 0),
    ] {
        let pos = position_of(source, name);
        assert_eq!(
            definition_locations(
                goto_definition_at(&backend, &uri, pos.line, pos.character + 2).await
            )
            .len(),
            expected,
            "{name}"
        );
    }
}

#[tokio::test]
async fn tempest_discovery_bootstrap_lifecycle_updates_warm_catalogues() {
    let source = "<x-card />";
    for entry in ["tempest", "public/index.php"] {
        let (backend, dir) = create_psr4_workspace(
            r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
            &[
                ("app/page.view.php", source),
                ("one/x-card.view.php", "One"),
                ("two/x-card.view.php", "Two"),
            ],
        );
        let uri = workspace_uri(&backend, "app/page.view.php");
        open_php(&backend, &uri, source).await;
        assert!(definition_locations(goto_definition_at(&backend, &uri, 0, 3).await).is_empty());
        let path = dir.path().join(entry);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bootstrap_uri = Uri::from_file_path(&path).unwrap();
        for (kind, component) in [
            (FileChangeType::Created, Some("one/x-card.view.php")),
            (FileChangeType::Changed, Some("two/x-card.view.php")),
            (FileChangeType::Deleted, None),
        ] {
            let expected = component.map(|component| {
                let component_path = dunce::canonicalize(dir.path().join(component)).unwrap();
                let literal = phpantom_lsp::php_type::PhpType::literal_string_value(
                    component_path.to_string_lossy(),
                );
                std::fs::write(
                    &path,
                    format!(
                        "<?php (new \\Tempest\\View\\ViewConfig())->addViewComponent({literal});"
                    ),
                )
                .unwrap();
                Uri::from_file_path(component_path).unwrap()
            });
            if component.is_none() {
                std::fs::remove_file(&path).unwrap();
            }
            backend
                .did_change_watched_files(DidChangeWatchedFilesParams {
                    changes: vec![FileEvent {
                        uri: bootstrap_uri.clone(),
                        kind,
                    }],
                })
                .await;
            let locations = definition_locations(goto_definition_at(&backend, &uri, 0, 3).await);
            assert_eq!(
                locations
                    .into_iter()
                    .map(|location| location.uri)
                    .collect::<Vec<_>>(),
                expected.into_iter().collect::<Vec<_>>(),
                "{entry}: {kind:?}"
            );
        }
    }
}
