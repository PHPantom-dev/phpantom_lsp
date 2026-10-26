use std::sync::Arc;

use crate::symbol_map::StringKeyKind;

/// A backend whose workspace holds `files`, every one published the way
/// the workspace index publishes it.
fn backend_with(files: &[(&str, &str)]) -> (crate::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    for (path, content) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
        let uri = crate::util::path_to_uri(&full);
        backend
            .open_files
            .write()
            .insert(uri.clone(), Arc::new(content.to_string()));
        if backend.is_resource_uri(&uri) {
            backend.update_resource_symbol_index(&uri, content);
        } else {
            backend.update_ast(&uri, content);
        }
    }
    (backend, dir)
}

#[test]
fn declarations_are_read_from_yaml_xml_and_php_configurators() {
    let (backend, _dir) = backend_with(&[
        (
            "config/services.yaml",
            "parameters:\n    app.sender: x\nservices:\n    app.mailer: ~\n",
        ),
        (
            "config/services.xml",
            "<container><services><service id=\"app.xml_mailer\"/></services></container>\n",
        ),
        (
            "config/services.php",
            "<?php\nnamespace Symfony\\Component\\DependencyInjection\\Loader\\Configurator;\n\nreturn static function (ContainerConfigurator $container): void {\n    $container->services()->set('app.php_mailer');\n    $container->parameters()->set('app.php_sender', 'x');\n};\n",
        ),
    ]);

    // The configurator calls are confirmed by the type engine, which needs
    // the configurator classes; without them only the configuration files
    // declare anything.
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::ServiceId),
        vec!["app.mailer".to_string(), "app.xml_mailer".to_string()]
    );
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::ContainerParameter),
        vec!["app.sender".to_string()]
    );
}

#[test]
fn a_file_that_stops_declaring_is_forgotten() {
    let (backend, dir) =
        backend_with(&[("config/services.yaml", "services:\n    app.mailer: ~\n")]);
    let uri = crate::util::path_to_uri(&dir.path().join("config/services.yaml"));
    backend.update_resource_symbol_index(&uri, "services:\n");
    assert!(
        backend
            .symfony_declared_names(StringKeyKind::ServiceId)
            .is_empty()
    );
}

#[test]
fn nothing_is_read_outside_a_symfony_project() {
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.set_is_symfony(false);
    let uri = crate::util::path_to_uri(&dir.path().join("config/services.yaml"));
    backend.update_resource_symbol_index(&uri, "services:\n    app.mailer: ~\n");
    assert!(
        backend
            .symfony_declared_names(StringKeyKind::ServiceId)
            .is_empty()
    );
}

#[test]
fn catalogues_nobody_opened_are_found_by_the_workspace_walk() {
    let dir = tempfile::tempdir().unwrap();
    let catalogues = [
        ("translations/messages.en.yaml", "demo:\n    title: Demo\n"),
        (
            "translations/validators.en.xlf",
            "<xliff version=\"1.2\"><file><body><trans-unit id=\"1\" resname=\"user.email\"><source>user.email</source></trans-unit></body></file></xliff>\n",
        ),
    ];
    for (path, content) in catalogues {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.ensure_workspace_indexed();

    let key = super::translations::translation_key;
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::SymfonyTranslation),
        vec![
            key("messages", "demo.title"),
            key("validators", "user.email")
        ]
    );
    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::TranslationDomain),
        vec!["messages".to_string(), "validators".to_string()]
    );
}

#[test]
fn templates_nobody_opened_are_found_by_the_workspace_walk() {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "templates/base.html.twig",
        "templates/demo/index.html.twig",
        "templates/bundles/AcmeBundle/widget.html.twig",
        "src/Resources/views/old.html.twig",
    ] {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, "<p></p>\n").unwrap();
    }
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.ensure_workspace_indexed();

    assert_eq!(
        backend.symfony_declared_names(StringKeyKind::TwigTemplate),
        vec![
            "@Acme/widget.html.twig".to_string(),
            "base.html.twig".to_string(),
            "bundles/AcmeBundle/widget.html.twig".to_string(),
            "demo/index.html.twig".to_string(),
        ]
    );
}

#[test]
fn forms_validation_and_configuration_are_read_only_in_a_symfony_project() {
    let dir = tempfile::tempdir().unwrap();
    let backend = crate::Backend::new_test_with_workspace(dir.path().to_path_buf(), vec![]);
    backend.set_is_symfony(false);
    let mapped = |uri: &str| {
        backend.symbol_map_for(uri).is_some_and(|map| {
            map.spans.iter().any(|span| {
                matches!(
                    &span.kind,
                    crate::symbol_map::SymbolKind::MemberAccess {
                        subject_text: crate::symbol_map::SubjectText::Owned(_),
                        ..
                    } | crate::symbol_map::SymbolKind::StringKey { .. }
                )
            })
        })
    };

    let validation = crate::util::path_to_uri(&dir.path().join("config/validator/User.yaml"));
    backend.update_resource_symbol_index(
        &validation,
        "App\\Entity\\User:\n    properties:\n        email:\n            - NotBlank: ~\n",
    );
    assert!(!mapped(&validation));

    let package = crate::util::path_to_uri(&dir.path().join("config/packages/acme.yaml"));
    backend.update_resource_symbol_index(&package, "acme:\n    api_key: x\n");
    assert!(!mapped(&package));

    let configuration = crate::util::path_to_uri(&dir.path().join("src/Configuration.php"));
    backend.update_ast(
        &configuration,
        "<?php\nuse Symfony\\Component\\Config\\Definition\\Builder\\TreeBuilder;\nclass Configuration {\n    public function getConfigTreeBuilder() {\n        $tb = new TreeBuilder('acme');\n        $tb->getRootNode()->children()->scalarNode('api_key')->end();\n        return $tb;\n    }\n}\n",
    );
    assert!(!mapped(&configuration));
}
