use super::*;
use crate::test_fixtures::{
    make_class, make_method, make_method_with_params, make_param, no_loader,
};
use crate::type_engine::resolver::Loaders;
use crate::types::TraitPrecedence;
use crate::virtual_members::laravel::ELOQUENT_BUILDER_FQN;

fn context<'a>(
    class: &'a ClassInfo,
    content: &'a str,
    loader: &'a dyn Fn(&str) -> Option<Arc<ClassInfo>>,
) -> ForwardWalkCtx<'a> {
    ForwardWalkCtx {
        current_class: class,
        all_classes: &[],
        content,
        cursor_offset: content.len() as u32,
        class_loader: loader,
        source_class_loader: loader,
        source_imports: None,
        backend: None,
        loaders: Loaders::default(),
        resolved_class_cache: None,
        enclosing_return_type: None,
        top_level_scope: None,
        in_loop: false,
        template_markers: None,
        cursor_scope: None,
    }
}

#[test]
fn callback_declaration_ownership_respects_docblocks_trait_precedence_and_cycles() {
    let mut documented = make_class("Documented");
    documented.set_class_docblock(Some(
        "/** @method void whereHas(string $relation, callable $callback) */".into(),
    ));
    assert_eq!(
        declared_relation_method(&documented, "whereHas", &no_loader, 0),
        Some(false)
    );

    let framework_name = "Illuminate\\Database\\Eloquent\\Concerns\\QueriesRelationships";
    let mut framework = make_class(framework_name);
    framework
        .methods
        .push(Arc::new(make_method("whereHas", None)));
    let mut excluded = make_class("Excluded");
    excluded
        .methods
        .push(Arc::new(make_method("whereHas", None)));
    let classes = [
        Arc::new(framework),
        Arc::new(excluded),
        Arc::new(make_class("Empty")),
    ];
    let loader = |name: &str| classes.iter().find(|class| class.fqn() == name).cloned();
    let mut owner = make_class("Owner");
    owner.used_traits = vec![atom("Excluded"), atom("Empty"), atom(framework_name)];
    owner.trait_precedences.push(TraitPrecedence {
        trait_name: atom(framework_name),
        method_name: atom("whereHas"),
        insteadof: vec![atom("Excluded")],
    });
    assert_eq!(
        declared_relation_method(&owner, "whereHas", &loader, 0),
        Some(true)
    );
    assert_eq!(declared_relation_method(&owner, "with", &loader, 0), None);

    let mut cycle = make_class("Cycle");
    cycle.parent_class = Some(atom("Cycle"));
    let cycle = Arc::new(cycle);
    let loader = |name: &str| (name == "Cycle").then(|| Arc::clone(&cycle));
    assert_eq!(
        declared_relation_method(&cycle, "whereHas", &loader, 0),
        Some(false)
    );
    let receiver = ResolvedType::from_class(owner.clone());
    assert!(!is_framework_relation_method(
        &receiver,
        &owner,
        "with",
        &context(&owner, "", &no_loader)
    ));
    let loader = |name: &str| (name == "Owner").then(|| Arc::new(owner.clone()));
    assert!(!is_framework_relation_method(
        &receiver,
        &owner,
        "with",
        &context(&owner, "", &loader)
    ));
}

#[test]
fn relation_callback_inference_rejects_incomplete_receivers_and_signatures() {
    let content = "<?php Query::whereHas('items', function ($query) {});";
    let empty = make_class("");
    for receiver in [
        ResolvedType::from_type_string(PhpType::parse("string")),
        ResolvedType::from_class(make_class(ELOQUENT_BUILDER_FQN)),
        {
            let mut builder = make_class(ELOQUENT_BUILDER_FQN);
            builder.methods.push(Arc::new(make_method_with_params(
                "whereHas",
                None,
                vec![
                    make_param("$items", Some("string"), true),
                    make_param("$handler", Some("Closure(int): void"), true),
                ],
            )));
            ResolvedType::from_class(builder)
        },
    ] {
        let loader = |name: &str| {
            receiver
                .class_info
                .as_ref()
                .filter(|class| class.fqn() == name)
                .cloned()
        };
        let ctx = context(&empty, content, &loader);
        let inferred = crate::parser::with_parsed_program(
            content,
            "incomplete_relation_callback",
            |program, _| {
                let call = program
                    .statements
                    .iter()
                    .find_map(|statement| match statement {
                        Statement::Expression(statement) => match statement.expression {
                            Expression::Call(Call::StaticMethod(call)) => Some(call),
                            _ => None,
                        },
                        _ => None,
                    })?;
                Some(
                    infer_relation_callback_params(
                        std::slice::from_ref(&receiver),
                        "whereHas",
                        1,
                        &call.argument_list,
                        &ScopeState::new(),
                        &ctx,
                    )
                    .is_none(),
                )
            },
        );
        assert_eq!(inferred, Some(true), "{:?}", receiver.type_string);
    }
}

#[test]
fn eager_callbacks_require_a_known_framework_receiver_and_relations_parameter() {
    let mut builder = make_class(ELOQUENT_BUILDER_FQN);
    builder.methods.push(Arc::new(make_method_with_params(
        "with",
        None,
        vec![make_param("$relations", None, true)],
    )));
    let raw = Arc::new(builder.clone());
    let empty = make_class("");
    let instance = "<?php $query->with(['items' => function ($item) {}]);";
    let static_call =
        "<?php Illuminate\\Database\\Eloquent\\Builder::with(['items' => function ($item) {}]);";
    for (content, receiver) in [
        (
            instance,
            ResolvedType::from_type_string(PhpType::parse("string")),
        ),
        (instance, ResolvedType::from_class(make_class("Unrelated"))),
        (
            static_call,
            ResolvedType::from_class(make_class(ELOQUENT_BUILDER_FQN)),
        ),
        (static_call, {
            let mut renamed = builder.clone();
            renamed.methods = vec![Arc::new(make_method_with_params(
                "with",
                None,
                vec![make_param("$loads", None, true)],
            ))]
            .into();
            ResolvedType::from_class(renamed)
        }),
    ] {
        let loader = |name: &str| (name == ELOQUENT_BUILDER_FQN).then(|| Arc::clone(&raw));
        // A local declaration or cached class can lag behind the loader's
        // current method metadata during an edit.
        let all_classes: Vec<_> = receiver.class_info.iter().cloned().collect();
        let cache = crate::virtual_members::new_resolved_class_cache();
        for class in &all_classes {
            crate::virtual_members::resolve_class_fully_cached(class, &no_loader, &cache);
        }
        let ctx = ForwardWalkCtx {
            all_classes: &all_classes,
            resolved_class_cache: Some(&cache),
            ..context(&empty, content, &loader)
        };
        let mut scope = ScopeState::new();
        scope.locals.insert(atom("$query"), vec![receiver]);
        assert_no_eager_context(content, &scope, &ctx);
    }
    let ctx = context(&empty, "", &no_loader);
    for content in [
        "<?php $query->{$method}(['items' => function ($item) {}]);",
        "<?php Missing::with(['items' => function ($item) {}]);",
        "<?php $class::with(['items' => function ($item) {}]);",
    ] {
        assert_no_eager_context(
            content,
            &ScopeState::new(),
            &ForwardWalkCtx {
                content,
                ..ctx.with_cursor_offset(content.len() as u32)
            },
        );
    }
}

fn assert_no_eager_context(content: &str, scope: &ScopeState, ctx: &ForwardWalkCtx<'_>) {
    let rejected =
        crate::parser::with_parsed_program(content, "unresolved_eager_callback", |program, _| {
            let call = program
                .statements
                .iter()
                .find_map(|statement| match statement {
                    Statement::Expression(statement) => match statement.expression {
                        Expression::Call(call) => Some(call),
                        _ => None,
                    },
                    _ => None,
                })?;
            Some(eager_callback_arguments(call, 0, scope, ctx).is_none())
        });
    assert_eq!(rejected, Some(true), "{content}");
}

/// A resolution context with no class or file around it, for the
/// relation query helpers.
fn resolution_context(loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>) -> ResolutionCtx<'_> {
    ResolutionCtx {
        current_class: None,
        all_classes: &[],
        content: "",
        cursor_offset: 0,
        class_loader: loader,
        source_class_loader: loader,
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

#[test]
fn relation_receivers_ignore_scalars_and_unrelated_classes() {
    let receivers = [
        ResolvedType::from_type_string(PhpType::parse("string")),
        ResolvedType::from_class(make_class("UnrelatedQuery")),
        ResolvedType::from_class(make_class(ELOQUENT_MODEL_FQN)),
    ];
    let ctx = resolution_context(&no_loader);
    let models = find_models_from_receivers(&receivers, &ctx);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].fqn(), ELOQUENT_MODEL_FQN);
    assert!(
        try_relation_query_override(&receivers, "map", &PhpType::parse("'items'"), None, &ctx)
            .is_none()
    );
    assert!(
        try_relation_query_override(
            &receivers[..2],
            "whereHas",
            &PhpType::parse("string"),
            None,
            &ctx
        )
        .is_none()
    );
}

#[test]
fn builder_receiver_recovers_model_from_instantiated_methods() {
    let mut model = make_class("App\\Product");
    model.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
    let model = Arc::new(model);
    let base = Arc::new(make_class(ELOQUENT_MODEL_FQN));
    let loader = |name: &str| match name.trim_start_matches('\\') {
        "App\\Product" => Some(Arc::clone(&model)),
        ELOQUENT_MODEL_FQN => Some(Arc::clone(&base)),
        _ => None,
    };
    for (method, return_type) in [
        ("getModel", "App\\Product"),
        (
            "where",
            "Illuminate\\Database\\Eloquent\\Builder<App\\Product>",
        ),
    ] {
        let mut builder = make_class(ELOQUENT_BUILDER_FQN);
        builder
            .methods
            .push(Arc::new(make_method(method, Some(return_type))));
        let models = find_models_from_receivers(
            &[ResolvedType::from_class(builder)],
            &resolution_context(&loader),
        );
        assert_eq!(models.len(), 1, "{method}");
        assert_eq!(models[0].fqn(), "App\\Product", "{method}");
    }
    assert!(
        find_models_from_receivers(
            &[ResolvedType::from_class(make_class(ELOQUENT_BUILDER_FQN))],
            &resolution_context(&loader),
        )
        .is_empty()
    );
}

#[test]
fn unresolved_morph_relations_use_base_builder_without_discarding_candidates() {
    let mut product = make_class("App\\Product");
    product.parent_class = Some(atom(ELOQUENT_MODEL_FQN));
    let product = Arc::new(product);
    let model = Arc::new(make_class(ELOQUENT_MODEL_FQN));
    let loader = |name: &str| match name {
        "App\\Product" => Some(Arc::clone(&product)),
        ELOQUENT_MODEL_FQN => Some(Arc::clone(&model)),
        _ => None,
    };
    let receivers = [ResolvedType::from_arc(Arc::clone(&model))];
    for (candidates, expected) in [
        (
            "'*'",
            "Illuminate\\Database\\Eloquent\\Builder<Illuminate\\Database\\Eloquent\\Model>",
        ),
        (
            "class-string<App\\Product>",
            "Illuminate\\Database\\Eloquent\\Builder<App\\Product>",
        ),
        (
            "class-string<App\\Product>|string",
            "Illuminate\\Database\\Eloquent\\Builder<App\\Product>|Illuminate\\Database\\Eloquent\\Builder<Illuminate\\Database\\Eloquent\\Model>",
        ),
    ] {
        assert_eq!(
            try_relation_query_override(
                &receivers,
                "whereHasMorph",
                &PhpType::parse("'missing'"),
                Some(&PhpType::parse(candidates)),
                &resolution_context(&loader)
            ),
            Some(vec![PhpType::parse(expected).simplified()]),
            "{candidates}",
        );
    }
    assert!(
        relation_callback_type(
            &model,
            "whereHas",
            &PhpType::parse("int"),
            &resolution_context(&loader)
        )
        .is_none()
    );
}
