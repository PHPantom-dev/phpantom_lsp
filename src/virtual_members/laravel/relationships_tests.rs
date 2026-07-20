use super::*;
use crate::php_type::PhpType;
use crate::test_fixtures::{make_class, make_method};

// ── classify_relationship ───────────────────────────────────────────

#[test]
fn classify_has_one() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("HasOne<Profile, $this>")),
        Some(RelationshipKind::Singular)
    );
}

#[test]
fn classify_has_many() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("HasMany<Post, $this>")),
        Some(RelationshipKind::Collection)
    );
}

#[test]
fn classify_belongs_to() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("BelongsTo<User, $this>")),
        Some(RelationshipKind::Singular)
    );
}

#[test]
fn classify_belongs_to_many() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("BelongsToMany<Role, $this>")),
        Some(RelationshipKind::Collection)
    );
}

#[test]
fn classify_morph_one() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("MorphOne<Image, $this>")),
        Some(RelationshipKind::Singular)
    );
}

#[test]
fn classify_morph_many() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("MorphMany<Comment, $this>")),
        Some(RelationshipKind::Collection)
    );
}

#[test]
fn classify_morph_to() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("MorphTo")),
        Some(RelationshipKind::MorphTo)
    );
}

#[test]
fn classify_morph_to_many() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("MorphToMany<Tag, $this>")),
        Some(RelationshipKind::Collection)
    );
}

#[test]
fn classify_has_many_through() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("HasManyThrough<Post, Country>")),
        Some(RelationshipKind::Collection)
    );
}

#[test]
fn classify_fqn_relationship() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse(
            "Illuminate\\Database\\Eloquent\\Relations\\HasMany<Post, $this>"
        )),
        Some(RelationshipKind::Collection)
    );
}

#[test]
fn classify_non_relationship() {
    assert_eq!(classify_relationship_typed(&PhpType::parse("string")), None);
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("Collection<User>")),
        None,
    );
}

#[test]
fn classify_custom_fqn_has_many_is_not_relationship() {
    // A user class whose short name collides with an Eloquent relationship
    // should NOT be classified as a relationship.
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("App\\Relations\\HasMany<Post>")),
        None,
    );
}

#[test]
fn classify_custom_fqn_belongs_to_is_not_relationship() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("MyApp\\Custom\\BelongsTo<User>")),
        None,
    );
}

#[test]
fn classify_eloquent_fqn_without_leading_backslash() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse(
            "Illuminate\\Database\\Eloquent\\Relations\\HasOne<Profile, $this>"
        )),
        Some(RelationshipKind::Singular)
    );
}

#[test]
fn classify_eloquent_fqn_morph_to() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse(
            "Illuminate\\Database\\Eloquent\\Relations\\MorphTo"
        )),
        Some(RelationshipKind::MorphTo)
    );
}

#[test]
fn classify_bare_name_without_generics() {
    assert_eq!(
        classify_relationship_typed(&PhpType::parse("HasMany")),
        Some(RelationshipKind::Collection)
    );
}

// ── extract_related_type ────────────────────────────────────────────

#[test]
fn extracts_first_generic_arg() {
    let parsed = PhpType::parse("HasMany<Post, $this>");
    assert_eq!(
        extract_related_type_typed(&parsed),
        Some(&PhpType::Named("Post".to_string()))
    );
}

#[test]
fn extracts_fqn_related_type() {
    let parsed = PhpType::parse("HasOne<\\App\\Models\\Profile, $this>");
    assert_eq!(
        extract_related_type_typed(&parsed),
        Some(&PhpType::Named("\\App\\Models\\Profile".to_string()))
    );
}

#[test]
fn returns_none_without_generics() {
    assert_eq!(extract_related_type_typed(&PhpType::parse("HasMany")), None);
}

// ── class_has_relation_method_ci ────────────────────────────────────

#[test]
fn relation_method_matches_case_insensitively() {
    let mut class = make_class("Order");
    class.methods.push(std::sync::Arc::new(make_method(
        "orderProducts",
        Some("HasMany<Product, $this>"),
    )));

    assert!(class_has_relation_method_ci(&class, "orderProducts"));
    assert!(class_has_relation_method_ci(&class, "orderproducts"));
    assert!(class_has_relation_method_ci(&class, "ORDERPRODUCTS"));
}

#[test]
fn non_relation_method_is_not_matched() {
    let mut class = make_class("Order");
    class
        .methods
        .push(std::sync::Arc::new(make_method("total", Some("int"))));

    assert!(!class_has_relation_method_ci(&class, "total"));
    assert!(!class_has_relation_method_ci(&class, "missing"));
}

// ── build_property_type ─────────────────────────────────────────────

#[test]
fn singular_with_related() {
    assert_eq!(
        build_property_type(
            RelationshipKind::Singular,
            Some(&PhpType::Named("App\\Models\\Post".to_string())),
            None
        ),
        Some(PhpType::Named("App\\Models\\Post".to_string()))
    );
}

#[test]
fn singular_without_related() {
    assert_eq!(
        build_property_type(RelationshipKind::Singular, None::<&PhpType>, None),
        None
    );
}

#[test]
fn collection_with_related() {
    assert_eq!(
        build_property_type(
            RelationshipKind::Collection,
            Some(&PhpType::Named("App\\Models\\Post".to_string())),
            None
        ),
        Some(PhpType::Generic(
            "Illuminate\\Database\\Eloquent\\Collection".to_string(),
            vec![PhpType::Named("App\\Models\\Post".to_string())],
        ))
    );
}

#[test]
fn collection_without_related_uses_model() {
    assert_eq!(
        build_property_type(RelationshipKind::Collection, None::<&PhpType>, None),
        Some(PhpType::Generic(
            "Illuminate\\Database\\Eloquent\\Collection".to_string(),
            vec![PhpType::Named(
                "Illuminate\\Database\\Eloquent\\Model".to_string(),
            )],
        ))
    );
}

#[test]
fn morph_to_always_returns_model() {
    assert_eq!(
        build_property_type(
            RelationshipKind::MorphTo,
            Some(&PhpType::Named("App\\Models\\Foo".to_string())),
            None
        ),
        Some(PhpType::Named(
            "Illuminate\\Database\\Eloquent\\Model".to_string(),
        ))
    );
}

#[test]
fn collection_with_custom_collection() {
    assert_eq!(
        build_property_type(
            RelationshipKind::Collection,
            Some(&PhpType::Named("App\\Models\\Post".to_string())),
            Some("App\\Collections\\PostCollection")
        ),
        Some(PhpType::Generic(
            "App\\Collections\\PostCollection".to_string(),
            vec![PhpType::Named("App\\Models\\Post".to_string())],
        ))
    );
}

#[test]
fn collection_custom_collection_canonical() {
    assert_eq!(
        build_property_type(
            RelationshipKind::Collection,
            Some(&PhpType::Named("App\\Models\\Post".to_string())),
            Some("App\\Collections\\PostCollection")
        ),
        Some(PhpType::Generic(
            "App\\Collections\\PostCollection".to_string(),
            vec![PhpType::Named("App\\Models\\Post".to_string())],
        ))
    );
}

#[test]
fn singular_ignores_custom_collection() {
    assert_eq!(
        build_property_type(
            RelationshipKind::Singular,
            Some(&PhpType::Named("App\\Models\\Post".to_string())),
            Some("App\\Collections\\PostCollection")
        ),
        Some(PhpType::Named("App\\Models\\Post".to_string()))
    );
}

#[test]
fn morph_to_ignores_custom_collection() {
    assert_eq!(
        build_property_type(
            RelationshipKind::MorphTo,
            Some(&PhpType::Named("App\\Models\\Foo".to_string())),
            Some("App\\Collections\\FooCollection")
        ),
        Some(PhpType::Named(
            "Illuminate\\Database\\Eloquent\\Model".to_string(),
        ))
    );
}

// ── infer_relationship_from_body ────────────────────────────────────

#[test]
fn infer_has_many_from_body() {
    let body = "{ return $this->hasMany(Post::class); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasMany".to_string(),
            vec![PhpType::Named("Post".to_string())],
        ))
    );
}

#[test]
fn infer_has_one_from_body() {
    let body = "{ return $this->hasOne(Profile::class); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasOne".to_string(),
            vec![PhpType::Named("Profile".to_string())],
        ))
    );
}

#[test]
fn infer_belongs_to_from_body() {
    let body = "{ return $this->belongsTo(User::class); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\BelongsTo".to_string(),
            vec![PhpType::Named("User".to_string())],
        ))
    );
}

#[test]
fn infer_belongs_to_many_from_body() {
    let body = "{ return $this->belongsToMany(Role::class); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany".to_string(),
            vec![PhpType::Named("Role".to_string())],
        ))
    );
}

#[test]
fn infer_morph_one_from_body() {
    let body = "{ return $this->morphOne(Image::class, 'imageable'); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\MorphOne".to_string(),
            vec![PhpType::Named("Image".to_string())],
        ))
    );
}

#[test]
fn infer_morph_many_from_body() {
    let body = "{ return $this->morphMany(Comment::class, 'commentable'); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\MorphMany".to_string(),
            vec![PhpType::Named("Comment".to_string())],
        ))
    );
}

#[test]
fn infer_morph_to_from_body() {
    // morphTo never has a related model class argument.
    let body = "{ return $this->morphTo(); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Named(
            "\\Illuminate\\Database\\Eloquent\\Relations\\MorphTo".to_string(),
        ))
    );
}

#[test]
fn infer_morph_to_many_from_body() {
    let body = "{ return $this->morphToMany(Tag::class, 'taggable'); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\MorphToMany".to_string(),
            vec![PhpType::Named("Tag".to_string())],
        ))
    );
}

#[test]
fn infer_morphed_by_many_from_body() {
    let body = r#"return $this->morphedByMany(Tag::class, 'taggable');"#;
    let result = infer_relationship_from_body(body).unwrap();
    assert_eq!(
        result,
        PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\MorphToMany".to_string(),
            vec![PhpType::Named("Tag".to_string())],
        )
    );
}

#[test]
fn infer_has_many_through_from_body() {
    let body = "{ return $this->hasManyThrough(Post::class, Country::class); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasManyThrough".to_string(),
            vec![PhpType::Named("Post".to_string())],
        )),
    );
}

#[test]
fn infer_has_one_through_from_body() {
    let body = "{ return $this->hasOneThrough(Owner::class, Car::class); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasOneThrough".to_string(),
            vec![PhpType::Named("Owner".to_string())],
        ))
    );
}

#[test]
fn infer_relationship_fqn_class_argument() {
    let body = r"{ return $this->hasMany(\App\Models\Post::class); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasMany".to_string(),
            vec![PhpType::Named("Post".to_string())],
        ))
    );
}

#[test]
fn infer_relationship_with_extra_arguments() {
    let body = "{ return $this->hasMany(Post::class, 'user_id', 'id'); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasMany".to_string(),
            vec![PhpType::Named("Post".to_string())],
        ))
    );
}

#[test]
fn infer_relationship_with_whitespace() {
    let body = "{
        return $this->hasMany(  Post::class  );
    }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasMany".to_string(),
            vec![PhpType::Named("Post".to_string())],
        ))
    );
}

#[test]
fn infer_no_relationship_in_empty_body() {
    let body = "{ }";
    assert_eq!(infer_relationship_from_body(body), None);
}

#[test]
fn infer_no_relationship_for_non_relationship_call() {
    let body = "{ return $this->query(); }";
    assert_eq!(infer_relationship_from_body(body), None);
}

#[test]
fn infer_relationship_without_class_argument() {
    // Some projects use string-based relationship definitions.
    let body = "{ return $this->hasMany('App\\Models\\Post'); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Named(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasMany".to_string(),
        )),
        "Without ::class argument, returns bare FQN relationship name"
    );
}

#[test]
fn infer_morph_to_with_arguments() {
    // morphTo can optionally take a name and type column.
    let body = "{ return $this->morphTo('commentable', 'commentable_type', 'commentable_id'); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Named(
            "\\Illuminate\\Database\\Eloquent\\Relations\\MorphTo".to_string(),
        ))
    );
}

#[test]
fn infer_relationship_multiline_body() {
    let body = "{
        return $this
            ->hasMany(Post::class, 'author_id');
    }";
    // The needle `$this->hasMany(` won't match across a line break,
    // so this returns None.  This is an acceptable limitation
    // documented in the todo.
    assert_eq!(infer_relationship_from_body(body), None);
}

#[test]
fn infer_relationship_same_line_chain() {
    let body = "{ return $this->hasMany(Post::class)->latest(); }";
    assert_eq!(
        infer_relationship_from_body(body),
        Some(PhpType::Generic(
            "\\Illuminate\\Database\\Eloquent\\Relations\\HasMany".to_string(),
            vec![PhpType::Named("Post".to_string())],
        ))
    );
}

// ── extract_class_argument ──────────────────────────────────────────

#[test]
fn extract_simple_class_arg() {
    assert_eq!(
        extract_class_argument("Post::class)"),
        Some("Post".to_string())
    );
}

#[test]
fn extract_fqn_class_arg() {
    assert_eq!(
        extract_class_argument("\\App\\Models\\Post::class)"),
        Some("Post".to_string())
    );
}

#[test]
fn extract_class_arg_with_extra_args() {
    assert_eq!(
        extract_class_argument("Post::class, 'user_id', 'id')"),
        Some("Post".to_string())
    );
}

#[test]
fn extract_class_arg_with_whitespace() {
    assert_eq!(
        extract_class_argument("  Post::class  )"),
        Some("Post".to_string())
    );
}

#[test]
fn extract_class_arg_no_class_token() {
    assert_eq!(extract_class_argument("'App\\Models\\Post')"), None);
}

#[test]
fn extract_class_arg_no_closing_paren() {
    assert_eq!(extract_class_argument("Post::class"), None);
}

#[test]
fn extract_class_arg_empty() {
    assert_eq!(extract_class_argument(")"), None);
}

#[test]
fn extract_class_arg_class_in_second_arg_only() {
    // `::class` appears only after the first comma — should return None.
    assert_eq!(extract_class_argument("'taggable', Tag::class)"), None);
}

// ── count_property_to_relationship_method ───────────────────────────

#[test]
fn count_to_relationship_simple() {
    let mut user = make_class("App\\Models\\User");
    user.methods
        .push(Arc::new(make_method("posts", Some("HasMany<Post, $this>"))));
    assert_eq!(
        count_property_to_relationship_method(&user, "posts_count"),
        Some("posts".to_string())
    );
}

#[test]
fn count_to_relationship_camel_case() {
    let mut bakery = make_class("App\\Models\\Bakery");
    bakery.methods.push(Arc::new(make_method(
        "headBaker",
        Some("HasOne<Baker, $this>"),
    )));
    assert_eq!(
        count_property_to_relationship_method(&bakery, "head_baker_count"),
        Some("headBaker".to_string())
    );
}

#[test]
fn count_to_relationship_multi_word() {
    let mut model = make_class("App\\Models\\Order");
    model.methods.push(Arc::new(make_method(
        "masterRecipe",
        Some("BelongsToMany<Recipe, $this>"),
    )));
    assert_eq!(
        count_property_to_relationship_method(&model, "master_recipe_count"),
        Some("masterRecipe".to_string())
    );
}

#[test]
fn count_to_relationship_morph_to() {
    let mut comment = make_class("App\\Models\\Comment");
    comment
        .methods
        .push(Arc::new(make_method("commentable", Some("MorphTo"))));
    assert_eq!(
        count_property_to_relationship_method(&comment, "commentable_count"),
        Some("commentable".to_string())
    );
}

#[test]
fn count_to_relationship_returns_none_for_non_relationship() {
    let mut user = make_class("App\\Models\\User");
    user.methods
        .push(Arc::new(make_method("getName", Some("string"))));
    assert_eq!(
        count_property_to_relationship_method(&user, "get_name_count"),
        None
    );
}

#[test]
fn count_to_relationship_returns_none_without_suffix() {
    let mut user = make_class("App\\Models\\User");
    user.methods
        .push(Arc::new(make_method("posts", Some("HasMany<Post, $this>"))));
    assert_eq!(count_property_to_relationship_method(&user, "posts"), None);
}

#[test]
fn count_to_relationship_returns_none_for_bare_count() {
    let user = make_class("App\\Models\\User");
    assert_eq!(count_property_to_relationship_method(&user, "_count"), None);
}

#[test]
fn count_to_relationship_returns_none_when_method_missing() {
    let user = make_class("App\\Models\\User");
    assert_eq!(
        count_property_to_relationship_method(&user, "posts_count"),
        None
    );
}

// ── count_property_name ─────────────────────────────────────────────

#[test]
fn count_property_name_simple() {
    assert_eq!(count_property_name("posts"), "posts_count");
}

#[test]
fn count_property_name_camel_case() {
    assert_eq!(count_property_name("headBaker"), "head_baker_count");
}
