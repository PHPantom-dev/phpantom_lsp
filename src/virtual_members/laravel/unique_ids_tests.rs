use super::*;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::test_fixtures::{make_class, make_method, no_loader};
use crate::types::{ClassLikeKind, PropertySource, Visibility};
use crate::virtual_members::laravel::LaravelModelProvider;
use crate::virtual_members::laravel::database_schema::{SchemaIndex, parse_schema_dump};
use crate::virtual_members::{VirtualMemberProvider, VirtualMembers, new_resolved_class_cache};

const UUIDS: &str = "Illuminate\\Database\\Eloquent\\Concerns\\HasUuids";
const ULIDS: &str = "Illuminate\\Database\\Eloquent\\Concerns\\HasUlids";

fn model(traits: &[&str]) -> ClassInfo {
    let mut class = make_class("App\\Models\\User");
    class.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
    class.used_traits = traits.iter().map(|name| atom(name)).collect();
    class.laravel_mut();
    class
}

fn assert_key(members: &VirtualMembers, name: &str, expected: &str) {
    let keys: Vec<_> = members
        .properties
        .iter()
        .filter(|p| p.name == name)
        .collect();
    assert_eq!(keys.len(), 1, "expected one {name} property");
    assert_eq!(keys[0].type_hint_str().as_deref(), Some(expected));
    assert_eq!(keys[0].visibility, Visibility::Public);
    assert!(!keys[0].is_static);
}

#[test]
fn unique_ids_type_primary_keys_as_strings() {
    for trait_name in [
        UUIDS,
        ULIDS,
        "\\Illuminate\\Database\\Eloquent\\Concerns\\hasuuids",
    ] {
        let mut class = model(&[trait_name]);
        // The traits override the framework's default $keyType = 'int'.
        class.laravel_mut().key_type = Some("int".into());
        assert!(uses_unique_string_ids(&class, &no_loader));
        assert_key(
            &LaravelModelProvider.provide(&class, &no_loader, None),
            "id",
            "string",
        );

        class.laravel_mut().primary_key = Some("identifier".into());
        let members = LaravelModelProvider.provide(&class, &no_loader, None);
        assert_key(&members, "identifier", "string");
        assert!(members.properties.iter().all(|p| p.name != "id"));
    }
}

#[test]
fn unique_ids_follow_parent_models_and_composed_traits() {
    for trait_name in [UUIDS, ULIDS] {
        let mut nested = make_class("App\\Concerns\\Identifiers");
        nested.kind = ClassLikeKind::Trait;
        nested.used_traits = vec![atom(trait_name)];
        let nested = Arc::new(nested);
        let mut wrapper = make_class("App\\Concerns\\ModelTraits");
        wrapper.kind = ClassLikeKind::Trait;
        wrapper.used_traits = vec![atom("App\\Concerns\\Missing"), nested.name];
        let wrapper = Arc::new(wrapper);
        let mut parent = model(&[&wrapper.name]);
        parent.name = atom("App\\Models\\BaseModel");
        let parent = Arc::new(parent);
        let mut middle = model(&[]);
        middle.name = atom("App\\Models\\Intermediate");
        middle.parent_class = Some(parent.name);
        let middle = Arc::new(middle);
        let loader = |name: &str| {
            [&nested, &wrapper, &parent, &middle]
                .into_iter()
                .find(|class| class.name == name)
                .map(Arc::clone)
        };
        let mut class = model(&[]);
        class.parent_class = Some(middle.name);
        assert_key(
            &LaravelModelProvider.provide(&class, &loader, None),
            "id",
            "string",
        );
        class.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
        class.used_traits = vec![wrapper.name];
        assert_key(
            &LaravelModelProvider.provide(&class, &loader, None),
            "id",
            "string",
        );
    }
}

#[test]
fn unique_ids_do_not_match_unrelated_trait_names() {
    let class = model(&["HasUuids", "App\\HasUuids", "App\\HasUlids"]);
    assert!(!uses_unique_string_ids(&class, &no_loader));
    assert_key(
        &LaravelModelProvider.provide(&class, &no_loader, None),
        "id",
        "int",
    );
    let mut plain = make_class("PlainClass");
    plain.used_traits = vec![atom(UUIDS)];
    assert!(!LaravelModelProvider.applies_to(&plain, &no_loader));
}

#[test]
fn unique_ids_preserve_explicit_attribute_types() {
    for trait_name in [UUIDS, ULIDS] {
        let mut class = model(&[trait_name]);
        class.laravel_mut().casts_definitions = vec![("id".into(), "integer".into())];
        assert_key(
            &LaravelModelProvider.provide(&class, &no_loader, None),
            "id",
            "int",
        );
        class.laravel_mut().casts_definitions.clear();
        class.laravel_mut().attributes_definitions = vec![("id".into(), PhpType::null())];
        assert_key(
            &LaravelModelProvider.provide(&class, &no_loader, None),
            "id",
            "null",
        );
        class.laravel_mut().attributes_definitions.clear();
        class
            .methods
            .push(Arc::new(make_method("getIdAttribute", Some("int"))));
        assert_key(
            &LaravelModelProvider.provide(&class, &no_loader, None),
            "id",
            "int",
        );
    }
}

#[test]
fn unique_ids_preserve_schema_metadata_and_dynamic_key_names() {
    let mut class = model(&[UUIDS]);
    let cache = new_resolved_class_cache();
    cache.write().set_schema_index(SchemaIndex::from_tables(
        Some("primary".into()),
        parse_schema_dump("primary", "CREATE TABLE users (id varchar(36) NOT NULL);"),
    ));
    let members = LaravelModelProvider.provide(&class, &no_loader, Some(&cache));
    assert_key(&members, "id", "string");
    assert!(matches!(
        members
            .properties
            .iter()
            .find(|p| p.name == "id")
            .unwrap()
            .source,
        Some(PropertySource::DatabaseColumn { .. })
    ));
    class.laravel_mut().has_get_key_name_method = true;
    let members = LaravelModelProvider.provide(&class, &no_loader, None);
    assert!(members.properties.iter().all(|p| p.name != "id"));
}

#[test]
fn unique_ids_handle_missing_parents_and_trait_cycles() {
    let mut class = model(&[]);
    class.parent_class = None;
    assert!(!uses_unique_string_ids(&class, &no_loader));
    class.parent_class = Some(atom("Missing"));
    assert!(!uses_unique_string_ids(&class, &no_loader));

    let mut cycle = make_class("CyclicTrait");
    cycle.used_traits = vec![cycle.name];
    let cycle = Arc::new(cycle);
    let loader = |name: &str| (name == cycle.name).then(|| Arc::clone(&cycle));
    class.used_traits = vec![cycle.name, cycle.name];
    assert!(!uses_unique_string_ids(&class, &loader));
    class.used_traits.push(atom(ULIDS));
    assert!(uses_unique_string_ids(&class, &loader));
}

#[test]
fn unique_ids_bound_inheritance_and_trait_depth() {
    let mut parent = make_class("CyclicParent");
    parent.parent_class = Some(parent.name);
    let parent = Arc::new(parent);
    let mut class = model(&[]);
    class.parent_class = Some(parent.name);
    let loads = std::cell::Cell::new(0);
    assert!(!uses_unique_string_ids(&class, &|_| {
        loads.set(loads.get() + 1);
        Some(Arc::clone(&parent))
    }));
    assert_eq!(loads.get(), MAX_INHERITANCE_DEPTH);

    let traits: Vec<_> = (0..MAX_TRAIT_DEPTH)
        .map(|i| {
            let mut class = make_class(&format!("Trait{i}"));
            class.used_traits = vec![atom(&format!("Trait{}", i + 1))];
            Arc::new(class)
        })
        .collect();
    let loader = |name: &str| {
        traits
            .iter()
            .find(|class| class.name == name)
            .map(Arc::clone)
    };
    class.parent_class = None;
    class.used_traits = vec![traits[0].name];
    assert!(!uses_unique_string_ids(&class, &loader));
}
