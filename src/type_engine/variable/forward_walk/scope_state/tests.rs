//! Characterisation tests for the join of two scopes.
//!
//! These pin down what `merge_branch` does to one variable at a time, so a
//! rewrite of the join (one that touches only the keys a branch wrote, say)
//! has something narrower than the narrowing integration suites to fail.

use super::ScopeState;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::test_fixtures::{make_class, make_method};
use crate::types::{ClassInfo, ResolvedType};

fn typed(ty: PhpType) -> Vec<ResolvedType> {
    vec![ResolvedType::from_type_string(ty)]
}

fn type_strings(scope: &ScopeState, var: &str) -> Vec<String> {
    scope
        .get(var)
        .iter()
        .map(|rt| rt.type_string.to_string())
        .collect()
}

fn foo() -> PhpType {
    PhpType::named(atom("Foo"))
}

#[test]
fn a_variable_typed_the_same_on_both_paths_keeps_its_type() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::string()));
    let b = a.clone();

    a.merge_branch(&b);

    assert_eq!(type_strings(&a, "$x"), vec!["string"]);
}

#[test]
fn a_variable_typed_differently_on_each_path_carries_both_types() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::string()));
    let mut b = ScopeState::new();
    b.set("$x", typed(PhpType::int()));

    a.merge_branch(&b);

    let mut types = type_strings(&a, "$x");
    types.sort();
    assert_eq!(types, vec!["int", "string"]);
}

#[test]
fn a_variable_only_one_path_binds_is_kept_as_a_possible_type() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::string()));
    let mut b = ScopeState::new();
    b.set("$x", typed(PhpType::string()));
    b.set("$y", typed(PhpType::int()));

    a.merge_branch(&b);

    assert_eq!(type_strings(&a, "$y"), vec!["int"]);
}

#[test]
fn a_variable_untyped_on_one_path_is_untyped_at_the_join() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::string()));
    let mut b = ScopeState::new();
    b.set_empty("$x");

    a.merge_branch(&b);

    assert!(a.contains("$x"));
    assert!(a.get("$x").is_empty());
}

#[test]
fn an_unreachable_incoming_path_contributes_nothing() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::string()));
    let mut b = ScopeState::new();
    b.set("$x", typed(PhpType::int()));
    b.set("$y", typed(PhpType::int()));
    b.unreachable = true;

    a.merge_branch(&b);

    assert_eq!(type_strings(&a, "$x"), vec!["string"]);
    assert!(!a.contains("$y"));
    assert!(!a.unreachable);
}

#[test]
fn an_unreachable_receiving_scope_adopts_the_incoming_path() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::string()));
    a.unreachable = true;
    let mut b = ScopeState::new();
    b.set("$y", typed(PhpType::int()));

    a.merge_branch(&b);

    assert!(!a.contains("$x"));
    assert_eq!(type_strings(&a, "$y"), vec!["int"]);
    assert!(!a.unreachable);
}

#[test]
fn a_virtual_member_proven_on_one_path_does_not_survive_the_join() {
    let mut proven = make_method("shine", None);
    proven.is_virtual = true;
    let with_member = ClassInfo {
        methods: vec![std::sync::Arc::new(proven)].into(),
        ..make_class("Foo")
    };
    let mut a = ScopeState::new();
    a.set("$f", vec![ResolvedType::from_class(with_member)]);
    let mut b = ScopeState::new();
    b.set("$f", vec![ResolvedType::from_class(make_class("Foo"))]);

    a.merge_branch(&b);

    let types = a.get("$f");
    assert_eq!(types.len(), 1);
    let class = types[0]
        .class_info
        .as_ref()
        .expect("the join keeps the class");
    assert!(class.get_method("shine").is_none());
}

#[test]
fn an_exclusion_survives_the_join_only_when_both_paths_made_it() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::string()));
    a.record_exclusion("$x", &foo());
    let mut b = ScopeState::new();
    b.set("$x", typed(PhpType::int()));

    let mut one_sided = a.clone();
    one_sided.merge_branch(&b);
    assert!(!one_sided.ruled_out.contains_key(&atom("$x")));

    b.record_exclusion("$x", &foo());
    a.merge_branch(&b);
    assert_eq!(a.ruled_out.get(&atom("$x")), Some(&vec![foo()]));
}
