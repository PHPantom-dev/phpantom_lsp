use super::*;
use crate::test_fixtures::{make_class, no_loader};

/// Parse a receiver expression the way the resolver hands it to
/// [`chain_count`]: the text to the left of the final `->create()`.
fn count_of(receiver: &str) -> FactoryCount {
    chain_count(&SubjectExpr::parse(receiver))
}

// ── is_count_conditional_method ─────────────────────────────────────

#[test]
fn count_conditional_methods_are_create_and_make() {
    assert!(is_count_conditional_method("create"));
    assert!(is_count_conditional_method("createQuietly"));
    assert!(is_count_conditional_method("make"));
}

#[test]
fn one_and_many_methods_are_not_count_conditional() {
    for name in [
        "createOne",
        "createOneQuietly",
        "createMany",
        "createManyQuietly",
        "makeOne",
        "makeMany",
        "state",
        "count",
    ] {
        assert!(
            !is_count_conditional_method(name),
            "{name} should not be count-conditional"
        );
    }
}

// ── chain_count: single-model chains ────────────────────────────────

#[test]
fn bare_factory_call_builds_one() {
    assert_eq!(count_of("User::factory()"), FactoryCount::One);
}

#[test]
fn factory_with_state_array_builds_one() {
    assert_eq!(
        count_of("User::factory(['name' => 'Ada'])"),
        FactoryCount::One
    );
}

#[test]
fn factory_with_closure_builds_one() {
    assert_eq!(
        count_of("User::factory(fn () => ['name' => 'Ada'])"),
        FactoryCount::One
    );
}

#[test]
fn factory_with_null_builds_one() {
    assert_eq!(count_of("User::factory(null)"), FactoryCount::One);
}

#[test]
fn factory_with_variable_count_builds_one() {
    // A variable could hold state just as easily as an integer, so the
    // single-model branch is the safe reading.
    assert_eq!(count_of("User::factory($count)"), FactoryCount::One);
}

#[test]
fn factory_new_builds_one() {
    assert_eq!(count_of("UserFactory::new()"), FactoryCount::One);
}

#[test]
fn relationship_calls_do_not_set_a_count() {
    // `hasPosts(3)` takes a count for the *relationship*, not the factory.
    assert_eq!(count_of("User::factory()->hasPosts(3)"), FactoryCount::One);
    assert_eq!(
        count_of("User::factory()->forAuthor()->trashed()"),
        FactoryCount::One
    );
}

#[test]
fn count_null_clears_a_count() {
    assert_eq!(count_of("User::factory()->count(null)"), FactoryCount::One);
    assert_eq!(count_of("User::factory(3)->count(null)"), FactoryCount::One);
    assert_eq!(count_of("User::factory()->count(NULL)"), FactoryCount::One);
}

#[test]
fn count_without_arguments_builds_one() {
    assert_eq!(count_of("User::factory()->count()"), FactoryCount::One);
}

#[test]
fn non_call_receiver_builds_one() {
    assert_eq!(count_of("$factory"), FactoryCount::One);
    assert_eq!(count_of("$this->factory"), FactoryCount::One);
}

// ── chain_count: collection chains ──────────────────────────────────

#[test]
fn count_call_builds_many() {
    assert_eq!(count_of("User::factory()->count(3)"), FactoryCount::Many);
}

#[test]
fn count_with_variable_builds_many() {
    // `count(?int $count)` only takes an integer or null, so a variable
    // argument that is not literally `null` sets a count.
    assert_eq!(count_of("User::factory()->count($n)"), FactoryCount::Many);
}

#[test]
fn count_zero_builds_many() {
    // `count(0)` yields an empty collection, not a single model.
    assert_eq!(count_of("User::factory()->count(0)"), FactoryCount::Many);
}

#[test]
fn instance_times_builds_many() {
    assert_eq!(count_of("User::factory()->times(3)"), FactoryCount::Many);
}

#[test]
fn static_times_builds_many() {
    assert_eq!(count_of("UserFactory::times(3)"), FactoryCount::Many);
}

#[test]
fn integer_factory_argument_builds_many() {
    assert_eq!(count_of("User::factory(3)"), FactoryCount::Many);
}

#[test]
fn numeric_string_factory_argument_builds_many() {
    // Laravel gates on `is_numeric()`, which accepts numeric strings.
    assert_eq!(count_of("User::factory('3')"), FactoryCount::Many);
}

#[test]
fn count_survives_later_non_count_calls() {
    assert_eq!(
        count_of("User::factory()->count(3)->hasPosts(2)->trashed()"),
        FactoryCount::Many
    );
}

#[test]
fn last_count_call_wins() {
    assert_eq!(
        count_of("User::factory()->count(null)->count(2)"),
        FactoryCount::Many
    );
    assert_eq!(
        count_of("User::factory()->count(2)->count(null)"),
        FactoryCount::One
    );
}

#[test]
fn count_on_a_new_factory_instance_builds_many() {
    assert_eq!(count_of("new UserFactory()->count(3)"), FactoryCount::Many);
}

#[test]
fn a_new_expression_head_without_a_count_builds_one() {
    assert_eq!(count_of("new UserFactory()->state([])"), FactoryCount::One);
}

// ── factory_model_type ──────────────────────────────────────────────

#[test]
fn model_type_prefers_extends_generic() {
    let mut factory = make_class("UserFactory");
    factory.file_namespace = Some(atom("Database\\Factories"));
    factory.extends_generics = vec![(
        atom("Illuminate\\Database\\Eloquent\\Factories\\Factory"),
        vec![PhpType::named(atom("App\\Domain\\Person"))],
    )];

    assert_eq!(
        factory_model_type(&factory, &no_loader).map(|t| t.to_string()),
        Some("App\\Domain\\Person".to_string()),
        "the @extends annotation names the model outright"
    );
}

#[test]
fn model_type_falls_back_to_the_naming_convention() {
    let mut factory = make_class("UserFactory");
    factory.file_namespace = Some(atom("Database\\Factories"));

    let model = Arc::new(make_class("User"));
    let loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        (name == "App\\Models\\User").then(|| Arc::clone(&model))
    };

    assert_eq!(
        factory_model_type(&factory, &loader).map(|t| t.to_string()),
        Some("App\\Models\\User".to_string())
    );
}

#[test]
fn model_type_is_none_when_the_conventional_model_is_missing() {
    let mut factory = make_class("WidgetFactory");
    factory.file_namespace = Some(atom("Database\\Factories"));

    assert_eq!(factory_model_type(&factory, &no_loader), None);
}

#[test]
fn model_type_ignores_generics_for_other_parents() {
    let mut factory = make_class("UserFactory");
    factory.file_namespace = Some(atom("Database\\Factories"));
    factory.extends_generics = vec![(
        atom("Illuminate\\Support\\Collection"),
        vec![PhpType::named(atom("App\\Models\\Other"))],
    )];

    assert_eq!(
        factory_model_type(&factory, &no_loader),
        None,
        "only an @extends Factory<…> annotation names the model"
    );
}
