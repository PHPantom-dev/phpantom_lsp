use std::sync::Arc;

use super::BEFORE_CACHING;
use crate::Backend;
use crate::symbol_map::{StringKeyKind, SymbolKind, SymbolMap, SymbolSpan};

const URI: &str = "file:///phpantom-typed-receiver-cache.php";
const SOURCE: &str = "<?php\nfunction mailer(\\Psr\\Container\\ContainerInterface $container) {\n    $container->get('app.mailer');\n}\n";

const CLASS_URI: &str = "file:///phpantom-typed-receiver-mailer.php";
const CLASS: &str = "<?php\nclass Mailer { public function send(): int { return 1; } }\n";
const CLASS_RETYPED: &str =
    "<?php\nclass Mailer { public function send(): string { return ''; } }\n";

const SERVICES_URI: &str = "file:///config/services.yaml";

/// Note a services file that declares `app.mailer`.
fn declare_mailer(backend: &Backend) {
    let declaring = SymbolMap {
        spans: vec![SymbolSpan {
            start: 12,
            end: 22,
            kind: SymbolKind::StringKey {
                kind: StringKeyKind::ServiceId,
                key: "app.mailer".to_string(),
                is_write: true,
                is_optional: false,
            },
        }],
        ..Default::default()
    };
    backend.note_container_declarations(SERVICES_URI, Some(&declaring));
}

fn open(backend: &Backend, uri: &str, source: &str) {
    backend
        .open_files
        .write()
        .insert(uri.to_string(), Arc::new(source.to_string()));
    backend.update_ast(uri, source);
}

fn opened() -> (Backend, Arc<SymbolMap>) {
    let backend = Backend::new_headless();
    open(&backend, CLASS_URI, CLASS);
    open(&backend, URI, SOURCE);
    let map = backend.symbol_map_for(URI).unwrap();
    assert!(!map.typed_receiver_sites.is_empty());
    (backend, map)
}

#[test]
fn confirmed_spans_are_reused_until_a_receiver_may_be_retyped() {
    let (backend, map) = opened();
    let first = backend.typed_receiver_spans_for(URI, &map);
    assert!(Arc::ptr_eq(
        &first,
        &backend.typed_receiver_spans_for(URI, &map)
    ));

    open(&backend, CLASS_URI, CLASS_RETYPED);
    let retyped = backend.typed_receiver_spans_for(URI, &map);
    assert!(!Arc::ptr_eq(&first, &retyped));

    declare_mailer(&backend);
    assert!(!Arc::ptr_eq(
        &retyped,
        &backend.typed_receiver_spans_for(URI, &map)
    ));
}

#[test]
fn reading_the_drupal_project_again_retires_confirmed_spans() {
    let (backend, map) = opened();
    let confirmed = backend.typed_receiver_spans_for(URI, &map);
    backend.clear_drupal_web_roots();
    assert!(!Arc::ptr_eq(
        &confirmed,
        &backend.typed_receiver_spans_for(URI, &map)
    ));
}

#[test]
fn spans_confirmed_while_a_signature_changes_are_not_kept() {
    let (backend, map) = opened();
    BEFORE_CACHING.set(Some(|backend| open(backend, CLASS_URI, CLASS_RETYPED)));
    let confirmed = backend.typed_receiver_spans_for(URI, &map);
    assert!(BEFORE_CACHING.take().is_none());
    assert!(!Arc::ptr_eq(
        &confirmed,
        &backend.typed_receiver_spans_for(URI, &map)
    ));
}

#[test]
fn spans_confirmed_while_the_container_changes_are_not_kept() {
    let (backend, map) = opened();
    BEFORE_CACHING.set(Some(declare_mailer));
    let confirmed = backend.typed_receiver_spans_for(URI, &map);
    assert!(BEFORE_CACHING.take().is_none());
    assert!(!Arc::ptr_eq(
        &confirmed,
        &backend.typed_receiver_spans_for(URI, &map)
    ));
}
