use super::*;
use crate::test_fixtures::{make_class, make_method, no_loader};

fn context(loader: &dyn Fn(&str) -> Option<Arc<ClassInfo>>) -> ResolutionCtx<'_> {
    ResolutionCtx {
        current_class: None,
        all_classes: &[],
        content: "",
        cursor_offset: 0,
        class_loader: loader,
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
    let ctx = context(&no_loader);
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
    let loader = |name: &str| match name {
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
        let models =
            find_models_from_receivers(&[ResolvedType::from_class(builder)], &context(&loader));
        assert_eq!(models.len(), 1, "{method}");
        assert_eq!(models[0].fqn(), "App\\Product", "{method}");
    }
    assert!(
        find_models_from_receivers(
            &[ResolvedType::from_class(make_class(ELOQUENT_BUILDER_FQN))],
            &context(&loader),
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
                &context(&loader)
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
            &context(&loader)
        )
        .is_none()
    );
}
