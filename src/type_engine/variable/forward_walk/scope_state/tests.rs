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

#[test]
fn a_join_with_a_fork_only_changes_what_the_branch_wrote() {
    let mut a = ScopeState::new();
    for i in 0..200 {
        a.set(&format!("$v{i}"), typed(PhpType::string()));
    }
    let mut b = a.clone();
    b.set("$v7", typed(PhpType::int()));
    b.set("$new", typed(PhpType::int()));
    b.remove("$v8");

    a.merge_branch(&b);

    let mut joined = type_strings(&a, "$v7");
    joined.sort();
    assert_eq!(joined, vec!["int", "string"]);
    assert_eq!(type_strings(&a, "$new"), vec!["int"]);
    assert_eq!(type_strings(&a, "$v8"), vec!["string"]);
    assert_eq!(type_strings(&a, "$v100"), vec!["string"]);
    assert_eq!(a.locals.len(), 201);
}

#[test]
fn reassigning_a_variable_drops_the_proofs_that_read_it() {
    let mut scope = ScopeState::new();
    for i in 0..50 {
        scope.record_non_null_implication(&format!("$h{i}"), vec![atom(&format!("$o{i}"))]);
    }
    scope.record_non_null_implication("$period", vec![atom("$agreement->period")]);
    scope.record_exclusion("$agreement->owner", &foo());

    scope.invalidate_proofs("$agreement");

    assert!(!scope.non_null_implications.contains_key(&atom("$period")));
    assert!(!scope.ruled_out.contains_key(&atom("$agreement->owner")));
    assert_eq!(scope.non_null_implications.len(), 50);

    scope.invalidate_proofs("$o3");
    assert!(!scope.non_null_implications.contains_key(&atom("$h3")));
    assert!(scope.non_null_implications.contains_key(&atom("$h30")));
}

#[test]
fn a_proof_recorded_after_a_fork_is_still_found_by_its_variable() {
    let mut a = ScopeState::new();
    a.record_non_null_implication("$h", vec![atom("$x")]);
    let mut b = a.clone();
    b.record_non_null_implication("$g", vec![atom("$y")]);

    b.invalidate_proofs("$y");
    a.invalidate_proofs("$x");

    assert!(!b.non_null_implications.contains_key(&atom("$g")));
    assert!(b.non_null_implications.contains_key(&atom("$h")));
    assert!(!a.non_null_implications.contains_key(&atom("$h")));
}

#[test]
fn literals_from_every_path_join_into_one_entry() {
    let mut a = ScopeState::new();
    a.set("$x", typed(PhpType::literal_string_value("a")));
    for value in ["b", "c", "a"] {
        let mut b = ScopeState::new();
        b.set("$x", typed(PhpType::literal_string_value(value)));
        a.merge_branch(&b);
    }

    assert_eq!(type_strings(&a, "$x"), vec!["'a'|'b'|'c'"]);
}

#[test]
fn joining_all_paths_keeps_them_in_order() {
    let paths: Vec<ScopeState> = ["a", "b", "c", "b", "d"]
        .iter()
        .map(|value| {
            let mut scope = ScopeState::new();
            scope.set("$x", typed(PhpType::literal_string_value(value)));
            scope
        })
        .collect();

    let joined = ScopeState::join_all(paths).expect("there are paths to join");

    assert_eq!(type_strings(&joined, "$x"), vec!["'a'|'b'|'c'|'d'"]);
    assert!(ScopeState::join_all(Vec::new()).is_none());
}

#[test]
fn long_literal_unions_are_disjoint_only_without_a_shared_literal() {
    use super::proofs::types_are_disjoint;

    let strings = |range: std::ops::Range<usize>, extra: &[PhpType]| {
        let mut members: Vec<PhpType> = range
            .map(|i| PhpType::literal_string_value(format!("v{i}")))
            .collect();
        members.extend_from_slice(extra);
        typed(PhpType::union(members))
    };

    assert!(types_are_disjoint(
        &strings(0..20, &[]),
        &strings(20..40, &[])
    ));
    assert!(!types_are_disjoint(
        &strings(0..20, &[]),
        &strings(19..40, &[])
    ));
    // A non-literal alternative still meets the literals on the other side.
    assert!(!types_are_disjoint(
        &strings(0..20, &[PhpType::string()]),
        &strings(20..40, &[])
    ));
    assert!(types_are_disjoint(
        &strings(0..20, &[PhpType::int()]),
        &strings(20..40, &[PhpType::null()])
    ));
    assert!(!types_are_disjoint(
        &strings(0..20, &[PhpType::null()]),
        &typed(PhpType::nullable(PhpType::parse("'v40'|'v41'"))),
    ));
}

#[test]
fn unions_that_share_a_member_are_not_disjoint() {
    use super::proofs::types_are_disjoint;

    let union = |values: &[&str]| {
        typed(PhpType::union(
            values.iter().map(|v| PhpType::literal_int(*v)).collect(),
        ))
    };

    assert!(!types_are_disjoint(
        &union(&["1", "2"]),
        &union(&["2", "3"])
    ));
    assert!(!types_are_disjoint(
        &typed(PhpType::parse("?int")),
        &typed(PhpType::parse("?string")),
    ));
    assert!(types_are_disjoint(&union(&["1", "2"]), &union(&["3", "4"])));
}

#[test]
fn arrays_are_disjoint_only_when_no_value_fits_both() {
    use super::proofs::types_are_disjoint;

    let disjoint =
        |a: &str, b: &str| types_are_disjoint(&typed(PhpType::parse(a)), &typed(PhpType::parse(b)));

    assert!(!disjoint(
        "non-empty-array<int, 1|2>",
        "non-empty-array<int, 2|3>"
    ));
    assert!(!disjoint("array<int, 1>", "array<int, 2>"));
    assert!(disjoint("array{}", "non-empty-array<int, int>"));
    assert!(disjoint(
        "non-empty-array<int, int>",
        "non-empty-array<int, string>"
    ));
    assert!(disjoint("array{kind: 'a'}", "array{kind: 'b'}"));
}
