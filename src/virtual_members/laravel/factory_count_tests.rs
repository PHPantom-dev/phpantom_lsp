use super::*;
use crate::test_fixtures::{make_class, no_loader};
use std::cell::Cell;

const FACTORY_BASE_FQN: &str = "Illuminate\\Database\\Eloquent\\Factories\\Factory";

/// Parse a receiver expression the way the resolver hands it to
/// [`chain_count`]: the text to the left of the final `->create()`.
fn count_of(receiver: &str) -> FactoryCount {
    chain_count(&SubjectExpr::parse(receiver))
}

/// Parse an expression and read its receiver chain through the AST path.
fn count_ast_of(receiver: &str) -> FactoryCount {
    let content = format!("<?php {receiver};");
    crate::parser::with_parsed_program(&content, "factory_count_ast_test", |program, content| {
        let expression = program
            .statements
            .iter()
            .find_map(|statement| match statement {
                mago_syntax::cst::Statement::Expression(statement) => Some(statement.expression),
                _ => None,
            });
        chain_count_ast(
            expression.expect("test source should contain an expression"),
            content,
        )
    })
}

fn resolution_ctx(class_loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>) -> ResolutionCtx<'_> {
    ResolutionCtx {
        current_class: None,
        all_classes: &[],
        content: "",
        cursor_offset: 0,
        class_loader,
        source_class_loader: class_loader,
        source_imports: None,
        backend: None,
        laravel_macro_this_resolver: None,
        resolved_class_cache: None,
        function_loader: None,
        scope_var_resolver: None,
        is_in_static_method: false,
        preserve_static: false,
    }
}

fn eloquent_factory_result(name: &str) -> ResolvedType {
    let mut factory = make_class(name);
    factory.parent_class = Some(atom(FACTORY_BASE_FQN));
    ResolvedType::from_arc(Arc::new(factory))
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
fn factory_with_a_boolean_builds_one() {
    // `is_numeric(true)` is false, so this is state rather than a count.
    assert_eq!(count_of("User::factory(true)"), FactoryCount::One);
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
fn ast_chains_read_arguments_only_for_count_setters() {
    assert_eq!(
        count_ast_of("User::factory()->count(3)"),
        FactoryCount::Many
    );
    assert_eq!(
        count_ast_of("User::factory(3)->state(['first', 'second'])"),
        FactoryCount::Many
    );
    assert_eq!(
        count_ast_of("User::factory(3)->count(null)"),
        FactoryCount::One
    );
    assert_eq!(
        count_ast_of("User::factory(3)->count(count: null)"),
        FactoryCount::One
    );
    assert_eq!(count_ast_of("User::factory()->count()"), FactoryCount::One);
    assert_eq!(
        count_ast_of("User::factory()->count($count)"),
        FactoryCount::Unknown
    );
}

#[test]
fn ast_static_heads_read_arguments_only_for_factory() {
    assert_eq!(count_ast_of("User::factory(3)"), FactoryCount::Many);
    assert_eq!(count_ast_of("User::factory(count: 3)"), FactoryCount::Many);
    assert_eq!(count_ast_of("User::factory([])"), FactoryCount::One);
    assert_eq!(count_ast_of("User::factory($count)"), FactoryCount::Unknown);
    assert_eq!(count_ast_of("UserFactory::times(3)"), FactoryCount::Many);
    assert_eq!(
        count_ast_of("UserFactory::new(['first', 'second'])"),
        FactoryCount::One
    );
    assert_eq!(
        count_ast_of("UserFactory::forTesting(['first', 'second'])"),
        FactoryCount::Unknown
    );
}

// ── chain_count: chains that settle nothing ─────────────────────────

/// A receiver that is not a chain carries no visible count state.  The
/// caller leaves the declared return type alone rather than picking a
/// branch, so a factory held in a variable *with* a count set does not
/// become a single model.
#[test]
fn a_non_call_receiver_is_unknown() {
    assert_eq!(count_of("$factory"), FactoryCount::Unknown);
    assert_eq!(count_of("$this->factory"), FactoryCount::Unknown);
    assert_eq!(count_of("$factory->state([])"), FactoryCount::Unknown);
}

/// `new UserFactory(3)` sets a count through the constructor, and the
/// subject parser does not keep the arguments to tell it from
/// `new UserFactory()`.
#[test]
fn a_new_expression_head_is_unknown() {
    assert_eq!(count_of("new UserFactory()"), FactoryCount::Unknown);
    assert_eq!(
        count_of("new UserFactory()->state([])"),
        FactoryCount::Unknown
    );
}

/// A static call other than `factory()`/`times()`/`new()` hands back a
/// factory built somewhere else, so its count is not visible here.
#[test]
fn an_unrelated_static_head_is_unknown() {
    assert_eq!(
        count_of("UserFactory::forTesting()->state([])"),
        FactoryCount::Unknown
    );
}

/// A variable argument to `factory()` could be the integer Laravel reads
/// as a count or the array it reads as state, and guessing wrong would
/// swap a model for a collection.
#[test]
fn factory_with_a_non_literal_argument_is_unknown() {
    assert_eq!(count_of("User::factory($count)"), FactoryCount::Unknown);
    assert_eq!(
        count_of("User::factory(self::COUNT)"),
        FactoryCount::Unknown
    );
    assert_eq!(
        count_of("User::factory(count($rows))"),
        FactoryCount::Unknown
    );
}

// ── chain_count: collection chains ──────────────────────────────────

#[test]
fn count_call_builds_many() {
    assert_eq!(count_of("User::factory()->count(3)"), FactoryCount::Many);
}

#[test]
fn count_with_non_literal_argument_is_unknown_from_syntax() {
    for argument in ["$n", "self::COUNT", "count($rows)"] {
        assert_eq!(
            count_of(&format!("User::factory()->count({argument})")),
            FactoryCount::Unknown,
            "{argument} could resolve to either int or null"
        );
    }
}

#[test]
fn count_zero_builds_many() {
    // `count(0)` yields an empty collection, not a single model.
    assert_eq!(count_of("User::factory()->count(0)"), FactoryCount::Many);
}

#[test]
fn literal_count_arguments_do_not_resolve_their_type() {
    let should_not_resolve =
        || -> Option<PhpType> { panic!("literal count arguments settle from syntax") };

    for literal in ["3", "+2", "-2", "1_000", "0x10"] {
        assert_eq!(
            count_argument_count(Some(literal), &should_not_resolve),
            FactoryCount::Many,
            "{literal} is an integer literal"
        );
    }
    assert_eq!(
        count_argument_count(Some("null"), &should_not_resolve),
        FactoryCount::One
    );
}

#[test]
fn unresolved_and_non_integer_count_arguments_remain_unknown() {
    let resolutions = Cell::new(0);
    let unresolved = || {
        resolutions.set(resolutions.get() + 1);
        None
    };

    assert_eq!(
        count_argument_count(Some("$count"), &unresolved),
        FactoryCount::Unknown
    );
    assert_eq!(resolutions.get(), 1);

    assert_eq!(
        count_argument_count(Some("3.5"), &|| Some(PhpType::parse("float"))),
        FactoryCount::Unknown,
        "a numeric expression is not enough for count(?int)"
    );
}

#[test]
fn resolved_count_argument_type_selects_only_certain_branches() {
    let count_for =
        |type_name: &str| count_argument_count(Some("$count"), &|| Some(PhpType::parse(type_name)));

    assert_eq!(count_for("int"), FactoryCount::Many);
    assert_eq!(count_for("positive-int"), FactoryCount::Many);
    assert_eq!(count_for("null"), FactoryCount::One);
    assert_eq!(count_for("?int"), FactoryCount::Unknown);
    assert_eq!(count_for("int|null"), FactoryCount::Unknown);
    assert_eq!(count_for("mixed"), FactoryCount::Unknown);
}

#[test]
fn an_ordinary_count_call_never_resolves_its_argument_for_factory_state() {
    let receiver = vec![ResolvedType::from_arc(Arc::new(make_class(
        "Illuminate\\Database\\Query\\Builder",
    )))];
    let resolutions = Cell::new(0);
    let first_arg_type = || {
        resolutions.set(resolutions.get() + 1);
        Some(PhpType::string())
    };
    let ctx = resolution_ctx(&no_loader);

    assert_eq!(
        fluent_factory_count(&receiver, "count", Some("$column"), &first_arg_type, &ctx),
        None
    );
    assert_eq!(resolutions.get(), 0);
}

#[test]
fn a_non_factory_static_factory_call_never_resolves_its_argument() {
    let mut results = vec![ResolvedType::from_arc(Arc::new(make_class(
        "App\\Support\\Builder",
    )))];
    let resolutions = Cell::new(0);
    let first_arg_type = || {
        resolutions.set(resolutions.get() + 1);
        Some(PhpType::int())
    };
    let ctx = resolution_ctx(&no_loader);

    tag_static_factory_call(
        &mut results,
        "factory",
        Some("$value"),
        &first_arg_type,
        &ctx,
    );

    assert_eq!(resolutions.get(), 0);
    assert_eq!(results[0].factory_count, FactoryCount::Unknown);
}

#[test]
fn unrelated_static_calls_never_inspect_factory_state() {
    let mut results = vec![eloquent_factory_result("Database\\Factories\\UserFactory")];
    let should_not_resolve = || -> Option<PhpType> {
        panic!("an unrelated static method must not resolve its argument")
    };
    let ctx = resolution_ctx(&no_loader);

    tag_static_factory_call(
        &mut results,
        "configure",
        Some("$value"),
        &should_not_resolve,
        &ctx,
    );

    assert_eq!(results[0].factory_count, FactoryCount::Unknown);
}

#[test]
fn static_factory_state_is_resolved_once_and_only_factory_results_are_tagged() {
    let mut results = vec![
        ResolvedType::from_arc(Arc::new(make_class("App\\Support\\Builder"))),
        eloquent_factory_result("Database\\Factories\\UserFactory"),
        ResolvedType::from_type_string(PhpType::null()),
        eloquent_factory_result("Database\\Factories\\AdminFactory"),
    ];
    let resolutions = Cell::new(0);
    let first_arg_type = || {
        resolutions.set(resolutions.get() + 1);
        Some(PhpType::int())
    };
    let ctx = resolution_ctx(&no_loader);

    tag_static_factory_call(
        &mut results,
        "factory",
        Some("$count"),
        &first_arg_type,
        &ctx,
    );

    assert_eq!(resolutions.get(), 1);
    assert_eq!(results[0].factory_count, FactoryCount::Unknown);
    assert_eq!(results[1].factory_count, FactoryCount::Many);
    assert_eq!(results[2].factory_count, FactoryCount::Unknown);
    assert_eq!(results[3].factory_count, FactoryCount::Many);
}

#[test]
fn unknown_static_factory_state_leaves_factory_results_unchanged() {
    let mut first = eloquent_factory_result("Database\\Factories\\UserFactory");
    first.factory_count = FactoryCount::One;
    let mut second = eloquent_factory_result("Database\\Factories\\AdminFactory");
    second.factory_count = FactoryCount::Many;
    let mut results = vec![first, second];
    let resolutions = Cell::new(0);
    let first_arg_type = || {
        resolutions.set(resolutions.get() + 1);
        Some(PhpType::mixed())
    };
    let ctx = resolution_ctx(&no_loader);

    tag_static_factory_call(
        &mut results,
        "factory",
        Some("$count"),
        &first_arg_type,
        &ctx,
    );

    assert_eq!(resolutions.get(), 1);
    assert_eq!(results[0].factory_count, FactoryCount::One);
    assert_eq!(results[1].factory_count, FactoryCount::Many);
}

#[test]
fn static_times_and_new_set_state_without_resolving_arguments() {
    let should_not_resolve = || -> Option<PhpType> {
        panic!("times() and new() settle count state without argument types")
    };
    let ctx = resolution_ctx(&no_loader);

    let mut times = vec![eloquent_factory_result("Database\\Factories\\UserFactory")];
    tag_static_factory_call(
        &mut times,
        "times",
        Some("$count"),
        &should_not_resolve,
        &ctx,
    );
    assert_eq!(times[0].factory_count, FactoryCount::Many);

    let mut new = vec![eloquent_factory_result("Database\\Factories\\UserFactory")];
    tag_static_factory_call(
        &mut new,
        "new",
        Some("$attributes"),
        &should_not_resolve,
        &ctx,
    );
    assert_eq!(new[0].factory_count, FactoryCount::One);
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

/// A count-setting call is read where it stands, without needing to walk
/// back to the head of the chain.
#[test]
fn count_on_a_new_factory_instance_builds_many() {
    assert_eq!(count_of("new UserFactory()->count(3)"), FactoryCount::Many);
    assert_eq!(count_of("$factory->count(3)"), FactoryCount::Many);
}

/// The chain head can be something other than a static call — the legacy
/// global `factory()` helper, or an invoked callable.  Neither says
/// anything about the count, whatever it was handed.
#[test]
fn a_non_static_chain_head_is_unknown() {
    assert_eq!(count_of("factory(3)"), FactoryCount::Unknown);
    assert_eq!(count_of("$makeFactory()"), FactoryCount::Unknown);
    assert_eq!(count_of("$makeFactory()->state([])"), FactoryCount::Unknown);
}

/// The walk descends into a strictly smaller sub-expression each step, so
/// a chain far longer than anything real still finds the count it set.
#[test]
fn a_very_long_chain_still_finds_the_count() {
    let counted = |links: usize| {
        let mut expr = String::from("User::factory()->count(3)");
        for _ in 0..links {
            expr.push_str("->state([])");
        }
        count_of(&expr)
    };

    assert_eq!(counted(10), FactoryCount::Many);
    assert_eq!(counted(200), FactoryCount::Many);
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

#[test]
fn model_type_prefers_declared_property_over_convention() {
    let mut factory = make_class("DraftFactory");
    factory.file_namespace = Some(atom("Database\\Factories"));
    factory.parent_class = Some(atom(FACTORY_BASE_FQN));
    factory.laravel_mut().factory_model = Some(PhpType::named(atom("App\\Domain\\PublishedDraft")));

    let mut factory_base = make_class(FACTORY_BASE_FQN);
    factory_base.template_params = vec![atom("TModel")];
    let conventional_model = make_class("App\\Models\\Draft");
    let loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            FACTORY_BASE_FQN => Some(Arc::new(factory_base.clone())),
            "App\\Models\\Draft" => Some(Arc::new(conventional_model.clone())),
            _ => None,
        }
    };

    assert_eq!(
        factory_model_type(&factory, &loader).map(|ty| ty.to_string()),
        Some("App\\Domain\\PublishedDraft".to_string())
    );
}

#[test]
fn model_type_prefers_forwarded_generic_over_declared_property() {
    let mut factory = make_class("DraftFactory");
    factory.parent_class = Some(atom("Database\\Factories\\DocumentFactory"));
    factory.extends_generics = vec![(
        atom("Database\\Factories\\DocumentFactory"),
        vec![PhpType::named(atom("App\\Domain\\PublishedDraft"))],
    )];
    factory.laravel_mut().factory_model = Some(PhpType::named(atom("App\\Models\\WrongDraft")));

    let mut document_factory = make_class("DocumentFactory");
    document_factory.file_namespace = Some(atom("Database\\Factories"));
    document_factory.parent_class = Some(atom(FACTORY_BASE_FQN));
    document_factory.template_params = vec![atom("TModel")];
    document_factory.extends_generics =
        vec![(atom(FACTORY_BASE_FQN), vec![PhpType::named(atom("TModel"))])];

    let mut factory_base = make_class(FACTORY_BASE_FQN);
    factory_base.template_params = vec![atom("TModel")];

    let loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Database\\Factories\\DocumentFactory" => Some(Arc::new(document_factory.clone())),
            FACTORY_BASE_FQN => Some(Arc::new(factory_base.clone())),
            _ => None,
        }
    };

    assert_eq!(
        factory_model_type(&factory, &loader).map(|ty| ty.to_string()),
        Some("App\\Domain\\PublishedDraft".to_string())
    );
}

// ── Count state carried by the value ────────────────────────────────

/// A resolved factory value with a count state attached.
fn factory_value(class: &str, count: FactoryCount) -> ResolvedType {
    let mut value = ResolvedType::from_arc(Arc::new(make_class(class)));
    value.factory_count = count;
    value
}

#[test]
fn a_lone_factory_value_speaks_for_itself() {
    assert_eq!(
        carried_count(&[factory_value("UserFactory", FactoryCount::Many)]),
        FactoryCount::Many
    );
}

#[test]
fn absent_or_unknown_factory_values_carry_nothing() {
    assert_eq!(carried_count(&[]), FactoryCount::Unknown);
    assert_eq!(
        carried_count(&[
            ResolvedType::from_type_string(PhpType::null()),
            factory_value("UserFactory", FactoryCount::Unknown),
            factory_value("AdminFactory", FactoryCount::Many),
        ]),
        FactoryCount::Unknown,
        "an unknown class-bearing alternative absorbs later count states"
    );
}

#[test]
fn factory_values_that_disagree_carry_nothing() {
    assert_eq!(
        carried_count(&[
            factory_value("UserFactory", FactoryCount::One),
            factory_value("UserFactory", FactoryCount::Many),
        ]),
        FactoryCount::Unknown
    );
}

/// The `null` half of a `?UserFactory` is not a factory of unknown
/// count, so it does not overrule the factory beside it.
#[test]
fn a_nullable_factory_keeps_its_count() {
    assert_eq!(
        carried_count(&[
            factory_value("UserFactory", FactoryCount::One),
            ResolvedType::from_type_string(PhpType::null()),
        ]),
        FactoryCount::One
    );
}

#[test]
fn fluent_calls_propagate_or_replace_carried_state_lazily() {
    let receiver = vec![factory_value("UserFactory", FactoryCount::One)];
    let should_not_resolve =
        || -> Option<PhpType> { panic!("this fluent call settles without resolving its argument") };
    let ctx = resolution_ctx(&no_loader);

    assert_eq!(
        fluent_factory_count(&receiver, "state", None, &should_not_resolve, &ctx),
        Some(FactoryCount::One)
    );
    assert_eq!(
        fluent_factory_count(
            &receiver,
            "times",
            Some("$count"),
            &should_not_resolve,
            &ctx,
        ),
        Some(FactoryCount::Many)
    );
    assert_eq!(
        fluent_factory_count(&receiver, "count", Some("null"), &should_not_resolve, &ctx,),
        Some(FactoryCount::One)
    );
}

#[test]
fn an_untracked_factory_receiver_is_proved_before_resolving_count() {
    let receiver = vec![eloquent_factory_result("Database\\Factories\\UserFactory")];
    let resolutions = Cell::new(0);
    let first_arg_type = || {
        resolutions.set(resolutions.get() + 1);
        Some(PhpType::int())
    };
    let ctx = resolution_ctx(&no_loader);

    assert_eq!(
        fluent_factory_count(&receiver, "count", Some("$count"), &first_arg_type, &ctx,),
        Some(FactoryCount::Many)
    );
    assert_eq!(resolutions.get(), 1);

    let should_not_resolve = || -> Option<PhpType> {
        panic!("a literal count settles without argument type resolution")
    };
    assert_eq!(
        fluent_factory_count(&receiver, "count", Some("3"), &should_not_resolve, &ctx,),
        Some(FactoryCount::Many)
    );
}

#[test]
fn an_unknown_non_count_receiver_propagates_nothing() {
    let receiver = vec![factory_value("UserFactory", FactoryCount::Unknown)];
    let should_not_resolve = || -> Option<PhpType> {
        panic!("ordinary fluent calls never inspect their arguments for count state")
    };
    let ctx = resolution_ctx(&no_loader);

    assert_eq!(
        fluent_factory_count(&receiver, "state", None, &should_not_resolve, &ctx),
        None
    );
}

#[test]
fn a_fluent_call_hands_its_state_to_the_same_class() {
    let receiver = vec![factory_value("UserFactory", FactoryCount::Many)];
    let mut results = vec![
        ResolvedType::from_arc(Arc::new(make_class("UserFactory"))),
        ResolvedType::from_arc(Arc::new(make_class("User"))),
    ];

    carry_factory_count(&mut results, &receiver, FactoryCount::Many);

    assert_eq!(results[0].factory_count, FactoryCount::Many);
    assert_eq!(
        results[1].factory_count,
        FactoryCount::Unknown,
        "the model a factory builds is not itself a factory"
    );
}

// ── factory($count) settled by the argument's type ──────────────────

#[test]
fn a_numeric_argument_type_sets_a_count() {
    for ty in ["int", "float", "positive-int"] {
        assert_eq!(
            numeric_argument_count(&PhpType::parse(ty)),
            FactoryCount::Many,
            "{ty} is what is_numeric() accepts"
        );
    }
}

#[test]
fn a_state_argument_type_sets_no_count() {
    for ty in ["array<string, mixed>", "callable", "Closure", "null"] {
        assert_eq!(
            numeric_argument_count(&PhpType::parse(ty)),
            FactoryCount::One,
            "{ty} is state, not a count"
        );
    }
}

#[test]
fn an_argument_type_that_could_be_either_settles_nothing() {
    for ty in ["mixed", "string", "int|array<mixed>"] {
        assert_eq!(
            numeric_argument_count(&PhpType::parse(ty)),
            FactoryCount::Unknown,
            "{ty} does not settle what is_numeric() would say"
        );
    }
}
