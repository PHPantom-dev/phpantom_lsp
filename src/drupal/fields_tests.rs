use std::sync::Arc;

use super::receiver_entity_types;
use crate::atom::atom;
use crate::class_lookup::Fqn;
use crate::types::{ClassInfo, ClassLikeKind};

/// A class or interface named by its FQN, with its parent and interfaces.
fn class_like(
    kind: ClassLikeKind,
    fqn: &str,
    parent: Option<&str>,
    interfaces: &[&str],
) -> Arc<ClassInfo> {
    let (namespace, name) = match fqn.rsplit_once('\\') {
        Some((namespace, name)) => (Some(atom(namespace)), name),
        None => (None, fqn),
    };
    Arc::new(ClassInfo {
        kind,
        name: atom(name),
        file_namespace: namespace,
        parent_class: parent.map(atom),
        interfaces: interfaces.iter().map(|name| atom(name)).collect(),
        ..Default::default()
    })
}

fn interface(fqn: &str, extends: &[&str]) -> Arc<ClassInfo> {
    class_like(ClassLikeKind::Interface, fqn, None, extends)
}

fn class(fqn: &str, parent: Option<&str>, interfaces: &[&str]) -> Arc<ClassInfo> {
    class_like(ClassLikeKind::Class, fqn, parent, interfaces)
}

/// Core's entity interfaces and two content entity types, `node` and
/// `user`, with `extra` beside them.
fn classes(extra: &[Arc<ClassInfo>]) -> Vec<Arc<ClassInfo>> {
    let mut classes = vec![
        interface("Drupal\\Core\\Entity\\EntityInterface", &[]),
        interface(
            "Drupal\\Core\\Entity\\FieldableEntityInterface",
            &["Drupal\\Core\\Entity\\EntityInterface"],
        ),
        interface(
            "Drupal\\Core\\Entity\\ContentEntityInterface",
            &["Drupal\\Core\\Entity\\FieldableEntityInterface"],
        ),
        class(
            "Drupal\\Core\\Entity\\ContentEntityBase",
            None,
            &["Drupal\\Core\\Entity\\ContentEntityInterface"],
        ),
        interface(
            "Drupal\\node\\NodeInterface",
            &["Drupal\\Core\\Entity\\ContentEntityInterface"],
        ),
        class(
            "Drupal\\node\\Entity\\Node",
            Some("Drupal\\Core\\Entity\\ContentEntityBase"),
            &["Drupal\\node\\NodeInterface"],
        ),
        interface(
            "Drupal\\user\\UserInterface",
            &["Drupal\\Core\\Entity\\ContentEntityInterface"],
        ),
        class(
            "Drupal\\user\\Entity\\User",
            Some("Drupal\\Core\\Entity\\ContentEntityBase"),
            &["Drupal\\user\\UserInterface"],
        ),
    ];
    classes.extend(extra.iter().cloned());
    classes
}

const ENTITY_TYPES: &[(&str, &str)] = &[
    ("node", "Drupal\\node\\Entity\\Node"),
    ("user", "Drupal\\user\\Entity\\User"),
];

/// The entity types the class or interface `receiver` holds, among
/// `entity_types`, with `classes` loadable.
fn entity_types_of(
    receiver: &str,
    classes: &[Arc<ClassInfo>],
    entity_types: &[(&'static str, &str)],
) -> Vec<&'static str> {
    let loader = |name: Fqn<'_>| {
        let name = name.as_str();
        let name = name.strip_prefix('\\').unwrap_or(name);
        classes.iter().find(|class| class.fqn() == name).cloned()
    };
    let receiver = loader(Fqn::new(receiver)).expect("receiver should be loadable");
    receiver_entity_types(
        &receiver,
        entity_types.iter().map(|(id, class)| (*id, atom(class))),
        &loader,
    )
}

#[test]
fn an_interface_one_entity_class_implements_holds_its_entity_type() {
    assert_eq!(
        entity_types_of("Drupal\\node\\NodeInterface", &classes(&[]), ENTITY_TYPES),
        vec!["node"]
    );
}

#[test]
fn an_interface_several_entity_classes_implement_holds_each_entity_type() {
    for receiver in [
        "Drupal\\Core\\Entity\\ContentEntityInterface",
        "Drupal\\Core\\Entity\\FieldableEntityInterface",
    ] {
        assert_eq!(
            entity_types_of(receiver, &classes(&[]), ENTITY_TYPES),
            vec!["node", "user"],
            "{receiver}"
        );
    }
}

#[test]
fn an_entity_class_holds_its_own_entity_type() {
    assert_eq!(
        entity_types_of("Drupal\\node\\Entity\\Node", &classes(&[]), ENTITY_TYPES),
        vec!["node"]
    );
}

#[test]
fn a_subclass_of_an_entity_class_holds_the_entity_type_it_extends() {
    let classes = classes(&[
        class(
            "Drupal\\acme\\Entity\\Bundle\\Article",
            Some("Drupal\\acme\\Entity\\Bundle\\Page"),
            &[],
        ),
        class(
            "Drupal\\acme\\Entity\\Bundle\\Page",
            Some("Drupal\\node\\Entity\\Node"),
            &[],
        ),
    ]);
    assert_eq!(
        entity_types_of(
            "Drupal\\acme\\Entity\\Bundle\\Article",
            &classes,
            ENTITY_TYPES
        ),
        vec!["node"]
    );
}

/// An entity class another entity type's class extends holds both, while
/// the subclass holds only its own.
#[test]
fn an_entity_class_with_entity_subclasses_holds_theirs_too() {
    let classes = classes(&[
        class(
            "Drupal\\acme\\Entity\\Note",
            Some("Drupal\\Core\\Entity\\ContentEntityBase"),
            &[],
        ),
        class(
            "Drupal\\acme\\Entity\\RevisionedNote",
            Some("Drupal\\acme\\Entity\\Note"),
            &[],
        ),
    ]);
    let entity_types = [
        ("acme_note", "Drupal\\acme\\Entity\\Note"),
        (
            "acme_revisioned_note",
            "Drupal\\acme\\Entity\\RevisionedNote",
        ),
    ];
    assert_eq!(
        entity_types_of("Drupal\\acme\\Entity\\Note", &classes, &entity_types),
        vec!["acme_note", "acme_revisioned_note"]
    );
    assert_eq!(
        entity_types_of(
            "Drupal\\acme\\Entity\\RevisionedNote",
            &classes,
            &entity_types
        ),
        vec!["acme_revisioned_note"]
    );
}

#[test]
fn a_receiver_without_fields_holds_no_entity_type() {
    for receiver in [
        "Drupal\\Core\\Entity\\EntityInterface",
        "Drupal\\acme\\Plain",
    ] {
        let classes = classes(&[class("Drupal\\acme\\Plain", None, &[])]);
        assert!(
            entity_types_of(receiver, &classes, ENTITY_TYPES).is_empty(),
            "{receiver}"
        );
    }
}

/// Only core's interface makes a receiver fieldable, not a global one that
/// shares its short name.
#[test]
fn a_global_interface_named_like_cores_is_not_fieldable() {
    let classes = classes(&[
        interface("FieldableEntityInterface", &[]),
        interface("Drupal\\acme\\NoteInterface", &["FieldableEntityInterface"]),
        class(
            "Drupal\\acme\\Entity\\Note",
            None,
            &["Drupal\\acme\\NoteInterface"],
        ),
    ]);
    assert!(
        entity_types_of(
            "Drupal\\acme\\NoteInterface",
            &classes,
            &[("acme_note", "Drupal\\acme\\Entity\\Note")]
        )
        .is_empty()
    );
}
