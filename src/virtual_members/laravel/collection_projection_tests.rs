use std::sync::Arc;

use super::*;
use crate::test_fixtures::{make_class, make_method, make_method_with_params, make_param};
use crate::type_engine::resolver::FileCtx;
use crate::type_engine::subject_expr::SubjectExpr;
use crate::types::ResolvedType;

#[test]
fn projection_specializes_returned_classes_without_a_hint_destination() {
    let mut collection = make_class(COLLECTION);
    collection.template_params = vec![atom("TKey"), atom("TValue")];
    collection.methods = vec![
        Arc::new(make_method_with_params(
            "pluck",
            Some("static<int, mixed>"),
            vec![make_param("value", None, true)],
        )),
        Arc::new(make_method("first", Some("TValue"))),
    ]
    .into();
    let collection = Arc::new(collection);
    let loader = |name: Fqn<'_>| (name.as_str() == COLLECTION).then(|| Arc::clone(&collection));
    let ctx = ResolutionCtx::new(
        FileCtx::new(
            "",
            &[],
            &loader,
            crate::test_fixtures::fqn_source_loader(&loader),
        ),
        None,
        0,
    );
    let classes = crate::Backend::resolve_call_return_types_on_receiver(
        &SubjectExpr::parse_callee("$items->pluck"),
        "'id'",
        Some(vec![ResolvedType::from_both_arc(
            PhpType::generic(
                COLLECTION,
                vec![PhpType::int(), PhpType::parse("array{id: int}")],
            ),
            Arc::clone(&collection),
        )]),
        &ctx,
        None,
    );
    assert_eq!(classes.len(), 1);
    assert_eq!(
        classes[0].get_method_ci("first").unwrap().return_type,
        Some(PhpType::int())
    );
}

#[test]
fn incomplete_collection_generics_keep_the_declared_return() {
    let owner = Arc::new(make_class(COLLECTION));
    let loader = |_: Fqn<'_>| Some(Arc::clone(&owner));
    let ctx = ResolutionCtx::new(
        FileCtx::new(
            "",
            &[],
            &loader,
            crate::test_fixtures::fqn_source_loader(&loader),
        ),
        None,
        0,
    );
    let incomplete = PhpType::generic(COLLECTION, vec![PhpType::string()]);
    for receiver in [None, Some(&incomplete)] {
        assert!(
            collection_property_return(
                &owner,
                receiver,
                ProjectionMethod::Pluck,
                &["'name'"],
                &ctx,
            )
            .is_none()
        );
    }
}

#[test]
fn selected_keys_follow_collection_coercions() {
    for (input, group, lazy, expected) in [
        ("int", false, false, "int"),
        ("7", false, false, "7"),
        ("'8'", false, false, "8"),
        ("'08'", false, false, "'08'"),
        ("'8'", false, true, "'8'"),
        ("true", false, false, "int"),
        ("false", false, false, "int"),
        ("1.5", false, false, "int"),
        ("string", false, false, "string"),
        ("bool", false, false, "int"),
        ("float", false, false, "int"),
        ("bool", false, true, "bool"),
        ("float", false, true, "float"),
        ("null", false, false, "''"),
        ("?int", false, false, "int|''"),
        ("int|string", false, false, "int|string"),
        ("mixed", false, false, "array-key"),
        ("array", true, false, "array-key"),
        ("list<string>", true, false, "string"),
        ("list<bool>", true, false, "int"),
        ("list<string>", false, false, "array-key"),
        ("Unknown", false, false, "array-key"),
    ] {
        assert_eq!(
            collection_key_type(&PhpType::parse(input), group, lazy, true, &|_| None),
            PhpType::parse(expected),
            "{input}"
        );
    }
}

#[test]
fn enum_and_stringable_group_keys_use_scalar_types() {
    let loader = |name: Fqn<'_>| {
        let mut class = make_class(name.as_str());
        match name.as_str() {
            "IntEnum" => {
                class.kind = ClassLikeKind::Enum;
                class.backed_type = Some(BackedEnumType::Int);
            }
            "StringEnum" => {
                class.kind = ClassLikeKind::Enum;
                class.backed_type = Some(BackedEnumType::String);
            }
            "UnitEnum" => class.kind = ClassLikeKind::Enum,
            "Label" => class
                .methods
                .push(Arc::new(make_method("__toString", Some("string")))),
            _ => {}
        }
        Some(Arc::new(class))
    };
    for (input, expected) in [
        ("IntEnum", "int"),
        ("StringEnum", "string"),
        ("UnitEnum", "string"),
        ("Label", "string"),
        ("Plain", "array-key"),
    ] {
        assert_eq!(
            collection_key_type(&PhpType::named(atom(input)), true, false, true, &loader),
            PhpType::parse(expected)
        );
    }
    // Pluck yields lazy keys directly, without keyBy/groupBy's enum conversion.
    for (input, lazy, expected) in [
        ("IntEnum", false, "array-key"),
        ("IntEnum", true, "IntEnum"),
        ("Label", true, "string"),
        ("?int", true, "int|null"),
    ] {
        assert_eq!(
            collection_key_type(&PhpType::parse(input), false, lazy, false, &loader),
            PhpType::parse(expected)
        );
    }
}
