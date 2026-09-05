use super::*;

/// Parse a type string with mago to get the canonical display form,
/// then parse with `PhpType::parse()` and verify our Display matches.
///
/// For types where mago's `Display` has a known bug (double angle
/// brackets on `class-string`, `interface-string`, `key-of`,
/// `value-of`), use [`assert_round_trip_expected`] instead.
fn assert_round_trip(input: &str) {
    // First, get mago's canonical output.
    let span = Span::new(
        FileId::zero(),
        Position::new(0),
        Position::new(input.len() as u32),
    );
    let arena = LocalArena::new();
    let mago_canonical = match mago_phpdoc_syntax::parse_type(&arena, input.as_bytes(), span) {
        Ok(ty) => ty.to_string(),
        Err(_) => {
            // If mago can't parse it, PhpType should produce Raw.
            let php_type = PhpType::parse(input);
            assert_eq!(
                php_type,
                PhpType::raw(input),
                "Unparseable input should become Raw"
            );
            return;
        }
    };

    let php_type = PhpType::parse(input);
    let our_output = php_type.to_string();
    assert_eq!(
        our_output, mago_canonical,
        "Round-trip mismatch for input: {input:?}\n  PhpType:  {php_type:?}\n  ours:     {our_output:?}\n  mago:     {mago_canonical:?}"
    );
}

/// Like [`assert_round_trip`] but compares against an explicit expected
/// string instead of mago's `Display` output.  Used to work around a
/// mago Display bug where `SingleGenericParameter` wraps the entry in
/// `<>` and then the parent type adds another pair, producing double
/// angle brackets (e.g. `class-string<<Foo>>`).
fn assert_round_trip_expected(input: &str, expected: &str) {
    let php_type = PhpType::parse(input);
    let our_output = php_type.to_string();
    assert_eq!(
        our_output, expected,
        "Round-trip mismatch for input: {input:?}\n  PhpType:  {php_type:?}\n  ours:     {our_output:?}\n  expected: {expected:?}"
    );
}

#[test]
fn round_trip_keywords() {
    let keywords = [
        "int",
        "string",
        "bool",
        "float",
        "mixed",
        "void",
        "null",
        "never",
        "true",
        "false",
        "object",
        "array",
        "callable",
        "iterable",
        "self",
        "static",
        "parent",
        "resource",
        "positive-int",
        "negative-int",
        "non-empty-string",
        "numeric-string",
        "array-key",
    ];
    for kw in keywords {
        assert_round_trip(kw);
    }
}

#[test]
fn round_trip_nullable() {
    assert_round_trip("?int");
    assert_round_trip("?string");
    assert_round_trip("?Foo");
}

#[test]
fn round_trip_union() {
    // mago Display uses spaced unions (`int | string`); we prefer
    // the PHP convention (`int|string`).
    assert_round_trip_expected("int|string", "int|string");
    assert_round_trip_expected("int|string|null", "int|string|null");
    assert_round_trip_expected("Foo|Bar|null", "Foo|Bar|null");
    assert_round_trip_expected("int|null", "int|null");
}

#[test]
fn round_trip_intersection() {
    // mago Display uses spaced intersections (`Countable & Traversable`);
    // we prefer the PHP convention (`Countable&Traversable`).
    assert_round_trip_expected("Countable&Traversable", "Countable&Traversable");
}

#[test]
fn round_trip_generics() {
    assert_round_trip("array<int, string>");
    assert_round_trip("array<string>");
    assert_round_trip("Collection<int, User>");
    assert_round_trip("list<int>");
    assert_round_trip("non-empty-list<string>");
    assert_round_trip("non-empty-array<string>");
}

#[test]
fn parse_generic_with_covariant_this() {
    // Laravel's relation annotations use `covariant $this` in generic args, e.g.
    // `BelongsTo<Category, covariant $this>`.  The parser should
    // still extract the base class name (`BelongsTo`) so that
    // member lookup works on the relationship class.
    let ty = PhpType::parse("BelongsTo<Category, covariant $this>");
    let base = ty.base_name();
    assert_eq!(
        base,
        Some("BelongsTo"),
        "base_name should be 'BelongsTo' even with 'covariant $this' arg, got: {:?} from {:?}",
        base,
        ty,
    );
}

#[test]
fn parse_generic_with_covariant_preserves_structure() {
    // The full Generic structure should be preserved after stripping.
    let ty = PhpType::parse("HasMany<Post, covariant $this>");
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let (name, args) = (&g.name, &g.args);
            assert_eq!(name, "HasMany");
            assert_eq!(args.len(), 2);
            assert_eq!(args[0].to_string(), "Post");
            assert_eq!(args[1].to_string(), "$this");
        }
        other => panic!("expected Generic, got: {:?}", other),
    }
}

#[test]
fn parse_generic_with_contravariant() {
    let ty = PhpType::parse("Comparator<contravariant T>");
    assert_eq!(
        ty.base_name(),
        Some("Comparator"),
        "base_name should work with contravariant annotation",
    );
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let args = &g.args;
            assert_eq!(args.len(), 1);
            assert_eq!(args[0].to_string(), "T");
        }
        other => panic!("expected Generic, got: {:?}", other),
    }
}

#[test]
fn parse_generic_with_covariant_fqn() {
    // Fully-qualified relationship type with covariant $this.
    let ty = PhpType::parse(
        "Illuminate\\Database\\Eloquent\\Relations\\BelongsTo<Category, covariant $this>",
    );
    assert_eq!(
        ty.base_name(),
        Some("Illuminate\\Database\\Eloquent\\Relations\\BelongsTo"),
    );
}

#[test]
fn parse_generic_with_multiple_covariant_args() {
    let ty = PhpType::parse("Map<covariant TKey, covariant TValue>");
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let (name, args) = (&g.name, &g.args);
            assert_eq!(name, "Map");
            assert_eq!(args.len(), 2);
            assert_eq!(args[0].to_string(), "TKey");
            assert_eq!(args[1].to_string(), "TValue");
        }
        other => panic!("expected Generic, got: {:?}", other),
    }
}

#[test]
fn parse_no_false_strip_of_covariant_class_name() {
    // A class named `covariant` (unlikely but possible) should not
    // be stripped when it is NOT inside a generic parameter position.
    // It appears at the top level, not after `<` or `,`.
    let ty = PhpType::parse("covariant");
    // mago may or may not parse this as a Named type; the key is
    // that stripping should NOT remove it since it's not after < or ,.
    assert_ne!(ty.to_string(), "", "should not produce empty string");
}

#[test]
fn parse_generic_without_covariant_unchanged() {
    // Normal generics without variance annotations should be unaffected.
    let ty = PhpType::parse("Collection<int, User>");
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let (name, args) = (&g.name, &g.args);
            assert_eq!(name, "Collection");
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected Generic, got: {:?}", other),
    }
}

#[test]
fn parse_covariant_array_shape_in_generic() {
    // `covariant array{...}` inside a generic — the array shape
    // should still parse after stripping the variance keyword.
    let ty = PhpType::parse(
        "Collection<int, covariant array{customer: Customer, contact: Contact|null}>",
    );
    assert_eq!(ty.base_name(), Some("Collection"));
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let args = &g.args;
            assert_eq!(args.len(), 2);
            // The second arg should be an array shape, not Raw.
            assert!(
                matches!(&args[1].kind(), TypeKind::ArrayShape(_)),
                "second arg should be ArrayShape after stripping covariant, got: {:?}",
                args[1],
            );
        }
        other => panic!("expected Generic, got: {:?}", other),
    }
}

#[test]
fn round_trip_class_references() {
    assert_round_trip("Foo\\Bar");
    assert_round_trip("\\Foo\\Bar");
}

#[test]
fn round_trip_shapes() {
    assert_round_trip("array{name: string, age: int}");
    assert_round_trip("array{0: string, 1: int}");
    assert_round_trip("array{name?: string}");
    assert_round_trip("object{name: string}");
}

#[test]
fn round_trip_callables() {
    assert_round_trip("callable(int, string): bool");
    assert_round_trip("Closure(int): void");
    assert_round_trip("Closure(int, string): void");
    assert_round_trip("callable(): void");
}

#[test]
fn round_trip_class_string() {
    // mago Display bug: class-string<Foo> → class-string<<Foo>>
    assert_round_trip_expected("class-string<Foo>", "class-string<Foo>");
    assert_round_trip("class-string");
}

#[test]
fn round_trip_interface_string() {
    // mago Display bug: interface-string<Foo> → interface-string<<Foo>>
    assert_round_trip_expected("interface-string<Foo>", "interface-string<Foo>");
}

#[test]
fn round_trip_key_of_value_of() {
    // mago Display bug: key-of<T> → key-of<<T>>, value-of<T> → value-of<<T>>
    assert_round_trip_expected("key-of<T>", "key-of<T>");
    assert_round_trip_expected("value-of<T>", "value-of<T>");
}

#[test]
fn round_trip_int_range() {
    // Mago's `Display` renders a range with `..` (`int<0..100>`); the
    // comma spelling is the one PHPStan and Psalm document and the one
    // developers write, so ours is compared against itself here.
    assert_round_trip_expected("int<0, 100>", "int<0, 100>");
    assert_round_trip_expected("int<min, max>", "int<min, max>");
    assert_round_trip_expected("int<0, max>", "int<0, max>");
}

#[test]
fn round_trip_slice() {
    // `Foo[]` is parsed as `PhpType::array_of(Foo)` which displays as `array<Foo>`.
    assert_round_trip_expected("Foo[]", "array<Foo>");
}

#[test]
fn round_trip_literals() {
    assert_round_trip("42");
    assert_round_trip("'foo'");
}

#[test]
fn round_trip_conditional() {
    assert_round_trip("$this is string ? int : float");
}

#[test]
fn round_trip_member_reference() {
    assert_round_trip("Foo::BAR");
    assert_round_trip("Foo::*");
}

#[test]
fn parse_generic_with_star_wildcard() {
    let ty = PhpType::parse("Relation<TRelatedModel, *, *>");
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let (name, args) = (&g.name, &g.args);
            assert_eq!(name, "Relation");
            assert_eq!(args.len(), 3);
            assert_eq!(args[0], PhpType::named(atom("TRelatedModel")));
            assert_eq!(args[1], PhpType::mixed());
            assert_eq!(args[2], PhpType::mixed());
        }
        other => panic!("Expected Generic, got {:?}", other),
    }
}

#[test]
fn parse_generic_with_star_wildcard_union() {
    // `Relation<TRelatedModel, *, *>|string` should parse as a union
    let ty = PhpType::parse("Relation<TRelatedModel, *, *>|string");
    match &ty.kind() {
        TypeKind::Union(members) => {
            assert_eq!(members.len(), 2);
            match &members[0].kind() {
                TypeKind::Generic(g) => {
                    let (name, args) = (&g.name, &g.args);
                    assert_eq!(name, "Relation");
                    assert_eq!(args.len(), 3);
                }
                other => panic!("Expected Generic, got {:?}", other),
            }
            assert_eq!(members[1], PhpType::string());
        }
        other => panic!("Expected Union, got {:?}", other),
    }
}

#[test]
fn parse_benevolent_unwraps_to_inner_union() {
    // PHPStan's `__benevolent<T>` wrapper parses as its inner type.
    let ty = PhpType::parse("__benevolent<Loop|null>");
    match &ty.kind() {
        TypeKind::Union(members) => {
            assert_eq!(members.len(), 2);
            assert_eq!(members[0], PhpType::named(atom("Loop")));
            assert_eq!(members[1], PhpType::null());
        }
        other => panic!("Expected Union, got {:?}", other),
    }
}

#[test]
fn parse_benevolent_unwraps_single_class() {
    let ty = PhpType::parse("__benevolent<Foo>");
    assert_eq!(ty, PhpType::named(atom("Foo")));
}

#[test]
fn parse_benevolent_nested_in_generic() {
    let ty = PhpType::parse("array<int, __benevolent<string|false>>");
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let (name, args) = (&g.name, &g.args);
            assert_eq!(name, "array");
            assert_eq!(args.len(), 2);
            match &args[1].kind() {
                TypeKind::Union(members) => {
                    assert_eq!(members.len(), 2);
                    assert_eq!(members[0], PhpType::string());
                    assert_eq!(members[1], PhpType::named(atom("false")));
                }
                other => panic!("Expected Union, got {:?}", other),
            }
        }
        other => panic!("Expected Generic, got {:?}", other),
    }
}

#[test]
fn parse_generic_star_does_not_mangle_member_reference() {
    // `Foo::*` is a member reference, not a generic wildcard
    assert_round_trip("Foo::*");
}

#[test]
fn parse_generic_star_does_not_mangle_constant_pattern() {
    // `int-mask-of<self::FOO_*>` — the `*` is part of a constant
    // pattern, not a bivariant wildcard, so it must survive parsing.
    assert_round_trip("int-mask-of<self::FOO_*>");
}

#[test]
fn parse_generic_star_with_spaces() {
    // Spaces around the `*` wildcard
    let ty = PhpType::parse("BelongsTo< * , * >");
    match &ty.kind() {
        TypeKind::Generic(g) => {
            let (name, args) = (&g.name, &g.args);
            assert_eq!(name, "BelongsTo");
            assert_eq!(args.len(), 2);
            assert_eq!(args[0], PhpType::mixed());
            assert_eq!(args[1], PhpType::mixed());
        }
        other => panic!("Expected Generic, got {:?}", other),
    }
}

#[test]
fn parse_generic_star_preserves_multibyte() {
    // A multibyte character alongside a bivariant wildcard must survive
    // intact (not be mangled byte-by-byte).
    assert_round_trip_expected("Map<Café, *>", "Map<Café, mixed>");
}

#[test]
fn variance_annotations_are_parsed_and_discarded() {
    // `covariant` / `contravariant` in a generic argument position carry
    // no meaning for our type model, so they are dropped, leaving the
    // annotated type behind (multibyte names intact).
    let ty = PhpType::parse("Map<café, covariant Naïve>");
    match &ty.kind() {
        TypeKind::Generic(g) => {
            assert_eq!(g.name, "Map");
            assert_eq!(g.args.len(), 2);
            assert_eq!(g.args[0].to_string(), "café");
            assert_eq!(g.args[1].to_string(), "Naïve");
        }
        other => panic!("Expected Generic, got {:?}", other),
    }
}

#[test]
fn parse_generic_wildcard_becomes_mixed() {
    assert_round_trip_expected(
        "Relation<TRelatedModel, *, *>",
        "Relation<TRelatedModel, mixed, mixed>",
    );
}

#[test]
fn parse_generic_star_keeps_member_wildcard() {
    // `self::*` is a member reference wildcard, not a bivariant `*`.
    assert_round_trip("Voter<self::*>");
}

#[test]
fn parse_empty_returns_raw() {
    assert_eq!(PhpType::parse(""), PhpType::untyped());
}

#[test]
fn parse_garbage_returns_raw() {
    let php_type = PhpType::parse("|||");
    assert!(matches!(php_type.kind(), TypeKind::Raw(_)));
}

#[test]
fn union_is_flattened() {
    let ty = PhpType::parse("int|string|null");
    match ty.kind() {
        TypeKind::Union(members) => {
            assert_eq!(members.len(), 3);
            assert_eq!(members[0], PhpType::int());
            assert_eq!(members[1], PhpType::string());
            assert_eq!(members[2], PhpType::null());
        }
        other => panic!("Expected Union, got {other:?}"),
    }
}

#[test]
fn intersection_is_flattened() {
    let ty = PhpType::parse("A&B&C");
    match ty.kind() {
        TypeKind::Intersection(members) => {
            assert_eq!(members.len(), 3);
            assert_eq!(members[0], PhpType::named(atom("A")));
            assert_eq!(members[1], PhpType::named(atom("B")));
            assert_eq!(members[2], PhpType::named(atom("C")));
        }
        other => panic!("Expected Intersection, got {other:?}"),
    }
}

#[test]
fn generic_with_params() {
    let ty = PhpType::parse("array<int, string>");
    match ty.kind() {
        TypeKind::Generic(g) => {
            let (name, args) = (&g.name, &g.args);
            assert_eq!(name, "array");
            assert_eq!(args.len(), 2);
            assert_eq!(args[0], PhpType::int());
            assert_eq!(args[1], PhpType::string());
        }
        other => panic!("Expected Generic, got {other:?}"),
    }
}

#[test]
fn class_string_with_param() {
    let ty = PhpType::parse("class-string<Foo>");
    match ty.kind() {
        TypeKind::ClassString(Some(inner)) => {
            assert_eq!(*inner, PhpType::named(atom("Foo")));
        }
        other => panic!("Expected ClassString(Some), got {other:?}"),
    }
}

#[test]
fn accepts_null_recognises_null_forms() {
    assert!(PhpType::parse("?int").accepts_null());
    assert!(PhpType::parse("int|null").accepts_null());
    assert!(PhpType::null().accepts_null());
    assert!(PhpType::mixed().accepts_null());
    assert!(!PhpType::int().accepts_null());
    assert!(!PhpType::parse("int|string").accepts_null());
}

#[test]
fn or_null_adds_null_and_is_idempotent() {
    assert_eq!(PhpType::int().or_null(), PhpType::parse("?int"));
    // Already nullable → unchanged.
    let nullable = PhpType::parse("?int");
    assert_eq!(nullable.clone().or_null(), nullable);
    // A union gains a null member rather than a nested wrapper.
    match PhpType::parse("int|string").or_null().kind() {
        TypeKind::Union(members) => {
            assert!(members.iter().any(|m| m.is_null()));
            assert_eq!(members.len(), 3);
        }
        other => panic!("Expected Union, got {other:?}"),
    }
}

#[test]
fn nullable_structure() {
    let ty = PhpType::parse("?int");
    match ty.kind() {
        TypeKind::Nullable(inner) => {
            assert_eq!(*inner, PhpType::int());
        }
        other => panic!("Expected Nullable, got {other:?}"),
    }
}

#[test]
fn callable_structure() {
    let ty = PhpType::parse("callable(int, string): bool");
    match ty.kind() {
        TypeKind::Callable(c) => {
            assert_eq!(c.kind, "callable");
            assert_eq!(c.params.len(), 2);
            assert_eq!(c.params[0].type_hint, PhpType::int());
            assert_eq!(c.params[1].type_hint, PhpType::string());
            assert_eq!(c.return_type, Some(PhpType::bool()));
        }
        other => panic!("Expected Callable, got {other:?}"),
    }
}

#[test]
fn shape_structure() {
    let ty = PhpType::parse("array{name: string, age?: int}");
    match ty.kind() {
        TypeKind::ArrayShape(entries) => {
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].key, Some("name".to_owned()));
            assert_eq!(entries[0].value_type, PhpType::string());
            assert!(!entries[0].optional);
            assert_eq!(entries[1].key, Some("age".to_owned()));
            assert_eq!(entries[1].value_type, PhpType::int());
            assert!(entries[1].optional);
        }
        other => panic!("Expected ArrayShape, got {other:?}"),
    }
}

#[test]
fn shape_value_type_named_key() {
    let ty = PhpType::parse("array{name: string, user: User}");
    assert_eq!(
        ty.shape_value_type("user"),
        Some(&PhpType::named(atom("User")))
    );
    assert_eq!(ty.shape_value_type("name"), Some(&PhpType::string()));
    assert_eq!(ty.shape_value_type("missing"), None);
}

#[test]
fn shape_value_type_positional() {
    let ty = PhpType::parse("array{User, Address}");
    assert_eq!(
        ty.shape_value_type("0"),
        Some(&PhpType::named(atom("User")))
    );
    assert_eq!(
        ty.shape_value_type("1"),
        Some(&PhpType::named(atom("Address")))
    );
    assert_eq!(ty.shape_value_type("2"), None);
}

#[test]
fn shape_value_type_explicit_numeric_key() {
    let ty = PhpType::parse("array{0: User, 1: Address}");
    assert_eq!(
        ty.shape_value_type("0"),
        Some(&PhpType::named(atom("User")))
    );
    assert_eq!(
        ty.shape_value_type("1"),
        Some(&PhpType::named(atom("Address")))
    );
}

#[test]
fn iterable_key_type_normalizes_explicit_numeric_shape_keys() {
    assert_eq!(
        PhpType::parse("array{0: User}")
            .iterable_key_type()
            .unwrap(),
        PhpType::int()
    );
    assert_eq!(
        PhpType::parse("array{0: User, name: string}")
            .iterable_key_type()
            .unwrap()
            .to_string(),
        "int|string"
    );
    assert_eq!(
        PhpType::parse("array{'0': User}")
            .iterable_key_type()
            .unwrap(),
        PhpType::int()
    );
    assert_eq!(
        PhpType::parse("array{'08': User}")
            .iterable_key_type()
            .unwrap(),
        PhpType::string()
    );
    // A quoted key is decoded the way PHP decodes the literal: `"\x38"` is
    // the string `8`, which PHP stores as the integer key `8`, while the
    // single-quoted spelling of the same characters decodes nothing and
    // stays a string key.
    assert_eq!(
        PhpType::parse(r#"array{"\x38": User}"#)
            .iterable_key_type()
            .unwrap(),
        PhpType::int()
    );
    assert_eq!(
        PhpType::parse(r#"array{'\x38': User}"#)
            .iterable_key_type()
            .unwrap(),
        PhpType::string()
    );
    assert_eq!(
        PhpType::parse(r#"array{'~\n~': string, '~\r~': string}"#)
            .iterable_key_type()
            .unwrap(),
        PhpType::string()
    );
}

/// A quoted shape key survives being displayed and parsed again: the
/// escapes the display writes are the ones PHP reads back.
#[test]
fn shape_keys_round_trip_through_their_display_form() {
    for key in [r"~\n~", "a'b", "a\"b", "with space", "\n", "\t"] {
        let shape = PhpType::array_shape(vec![ShapeEntry {
            key: Some(key.to_string()),
            value_type: PhpType::string(),
            optional: false,
        }]);
        let reparsed = PhpType::parse(&shape.to_string());
        assert_eq!(
            reparsed.shape_entries().and_then(|e| e[0].key.as_deref()),
            Some(key),
            "{}",
            shape
        );
    }
}

#[test]
fn iterable_key_type_makes_implicit_int_keys_explicit() {
    for spelling in [
        "list<User>",
        "non-empty-list<User>",
        "array<User>",
        "non-empty-array<User>",
        "User[]",
    ] {
        assert_eq!(
            PhpType::parse(spelling).iterable_key_type().unwrap(),
            PhpType::int(),
            "{spelling}"
        );
    }
    // A bare `array` names no key domain at all.
    assert_eq!(PhpType::parse("array").iterable_key_type(), None);
}

#[test]
fn iterable_key_type_keeps_object_shape_keys_as_strings() {
    for shape in [r#"object{'0': User}"#, r#"object{"\x38": User}"#] {
        assert_eq!(
            PhpType::parse(shape).iterable_key_type().unwrap(),
            PhpType::string(),
            "{shape}"
        );
    }
}

#[test]
fn iterable_key_type_preserves_unknown_class_constant_key_domain() {
    assert_eq!(
        PhpType::parse("array{Foo::BAR: User}")
            .iterable_key_type()
            .unwrap()
            .to_string(),
        "int|string"
    );
}

#[test]
fn shape_value_type_nullable() {
    let ty = PhpType::parse("?array{name: string}");
    assert_eq!(ty.shape_value_type("name"), Some(&PhpType::string()));
}

#[test]
fn shape_value_type_union_of_shapes() {
    // Union where only one member has the key (conditional shape addition).
    let ty = PhpType::parse("array{name: string}|array{name: string, config: Config}");
    assert_eq!(
        ty.shape_value_type("config"),
        Some(&PhpType::named(atom("Config")))
    );
    // Key present in both members returns the first match.
    assert_eq!(ty.shape_value_type("name"), Some(&PhpType::string()));
    // Key absent from all members.
    assert_eq!(ty.shape_value_type("missing"), None);
}

#[test]
fn shape_value_type_non_shape_returns_none() {
    assert_eq!(
        PhpType::parse("array<int, User>").shape_value_type("0"),
        None
    );
    assert_eq!(PhpType::parse("string").shape_value_type("0"), None);
}

#[test]
fn shape_entries_array() {
    let ty = PhpType::parse("array{name: string, age?: int}");
    let entries = ty.shape_entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key, Some("name".to_owned()));
    assert!(!entries[0].optional);
    assert_eq!(entries[1].key, Some("age".to_owned()));
    assert!(entries[1].optional);
}

#[test]
fn shape_entries_object() {
    let ty = PhpType::parse("object{foo: int}");
    let entries = ty.shape_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, Some("foo".to_owned()));
}

#[test]
fn shape_entries_non_shape_returns_none() {
    assert!(PhpType::parse("string").shape_entries().is_none());
    assert!(PhpType::parse("array<int>").shape_entries().is_none());
}

#[test]
fn is_array_shape_test() {
    assert!(PhpType::parse("array{name: string}").is_array_shape());
    assert!(PhpType::parse("?array{name: string}").is_array_shape());
    assert!(PhpType::parse("array{name: string}|null").is_array_shape());
    assert!(!PhpType::parse("array<int>").is_array_shape());
    assert!(!PhpType::parse("object{name: string}").is_array_shape());
}

#[test]
fn is_object_shape_test() {
    assert!(PhpType::parse("object{name: string}").is_object_shape());
    assert!(PhpType::parse("?object{name: string}").is_object_shape());
    assert!(!PhpType::parse("array{name: string}").is_object_shape());
    assert!(!PhpType::parse("string").is_object_shape());
}

#[test]
fn join_shapes_one_sided_key_becomes_optional() {
    let a = PhpType::parse("array{a: int}");
    let b = PhpType::parse("array{a: int, b: string}");
    assert_eq!(
        a.join_shapes(&b),
        Some(PhpType::parse("array{a: int, b?: string}"))
    );
    // Symmetric: the missing side still makes the key optional.
    assert_eq!(
        b.join_shapes(&a),
        Some(PhpType::parse("array{a: int, b?: string}"))
    );
}

#[test]
fn join_shapes_shared_key_unions_value_types() {
    let a = PhpType::parse("array{a: int}");
    let b = PhpType::parse("array{a: string}");
    assert_eq!(
        a.join_shapes(&b),
        Some(PhpType::parse("array{a: int|string}"))
    );
}

#[test]
fn join_shapes_identical_values_stay_single() {
    let a = PhpType::parse("array{a: int, b: string}");
    let b = PhpType::parse("array{a: int, b: string}");
    assert_eq!(a.join_shapes(&b), Some(a.clone()));
}

#[test]
fn join_shapes_optional_on_either_side_stays_optional() {
    let a = PhpType::parse("array{a?: int}");
    let b = PhpType::parse("array{a: int}");
    assert_eq!(a.join_shapes(&b), Some(PhpType::parse("array{a?: int}")));
}

#[test]
fn join_shapes_nested_shapes_join_recursively() {
    let a = PhpType::parse("array{data: array{x: int}}");
    let b = PhpType::parse("array{data: array{x: int, y: string}}");
    assert_eq!(
        a.join_shapes(&b),
        Some(PhpType::parse("array{data: array{x: int, y?: string}}"))
    );
}

#[test]
fn join_shapes_value_union_dedups_equivalent_members() {
    // int|string joined with string|float keeps each member once.
    let a = PhpType::parse("array{a: int|string}");
    let b = PhpType::parse("array{a: string|float}");
    assert_eq!(
        a.join_shapes(&b),
        Some(PhpType::parse("array{a: int|string|float}"))
    );
}

#[test]
fn join_shapes_nullable_side_makes_join_nullable() {
    let a = PhpType::parse("?array{a: int}");
    let b = PhpType::parse("array{a: int, b: string}");
    assert_eq!(
        a.join_shapes(&b),
        Some(PhpType::parse("?array{a: int, b?: string}"))
    );
}

#[test]
fn join_shapes_union_with_null_does_not_accumulate_variants() {
    let mut joined = PhpType::parse("array{a: int}|null");
    let b = PhpType::parse("array{a: int, b: string}|null");
    for _ in 0..100 {
        joined = joined.join_shapes(&b).unwrap();
    }
    assert_eq!(joined, PhpType::parse("?array{a: int, b?: string}"));
}

#[test]
fn join_shapes_pairs_an_appended_slot_by_index() {
    // The entry an append adds beside named keys occupies the index it
    // sits at, so the branch that did not append leaves it optional
    // rather than producing a second variant of the whole shape. The
    // index comes along, since a positional spelling would count the
    // entries after an absent one out at the wrong index.
    let appended = PhpType::parse("array{name: string, Pen}");
    assert_eq!(
        appended.join_shapes(&PhpType::parse("array{name: string}")),
        Some(PhpType::parse("array{name: string, 0?: Pen}"))
    );
    // Two bare lists of values describe unrelated arrays, so pairing
    // their slots up would invent a row neither one holds.
    let positional = PhpType::parse("array{int, string}");
    let keyed = PhpType::parse("array{a: int}");
    assert_eq!(
        positional.join_shapes(&PhpType::parse("array{float}")),
        None
    );
    assert_eq!(positional.join_shapes(&keyed), None);
    assert_eq!(keyed.join_shapes(&positional), None);
    // Non-shape types never join.
    assert_eq!(
        keyed.join_shapes(&PhpType::parse("array<int, string>")),
        None
    );
    assert_eq!(PhpType::string().join_shapes(&keyed), None);
}

#[test]
fn join_shapes_key_order_is_first_side_then_new_keys() {
    let a = PhpType::parse("array{b: int, a: int}");
    let b = PhpType::parse("array{c: int, a: int}");
    assert_eq!(
        a.join_shapes(&b),
        Some(PhpType::parse("array{b?: int, a: int, c?: int}"))
    );
}

#[test]
fn object_shape_property_type_test() {
    let ty = PhpType::parse("object{name: string, user: User}");
    assert_eq!(
        ty.object_shape_property_type("user"),
        Some(&PhpType::named(atom("User")))
    );
    assert_eq!(
        ty.object_shape_property_type("name"),
        Some(&PhpType::string())
    );
    assert_eq!(ty.object_shape_property_type("missing"), None);
}

#[test]
fn object_shape_structure() {
    let ty = PhpType::parse("object{name: string}");
    match ty.kind() {
        TypeKind::ObjectShape(entries) => {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].key, Some("name".to_owned()));
            assert_eq!(entries[0].value_type, PhpType::string());
        }
        other => panic!("Expected ObjectShape, got {other:?}"),
    }
}

#[test]
fn slice_structure() {
    let ty = PhpType::parse("Foo[]");
    match ty.kind() {
        TypeKind::Array(inner) => {
            assert_eq!(*inner, PhpType::named(atom("Foo")));
        }
        other => panic!("Expected Array (slice), got {other:?}"),
    }
}

#[test]
fn conditional_structure() {
    let ty = PhpType::parse("$this is string ? int : float");
    match ty.kind() {
        TypeKind::Conditional(c) => {
            assert_eq!(c.param, "$this");
            assert!(!c.negated);
            assert_eq!(c.condition, PhpType::string());
            assert_eq!(c.then_type, PhpType::int());
            assert_eq!(c.else_type, PhpType::float());
        }
        other => panic!("Expected Conditional, got {other:?}"),
    }
}

#[test]
fn int_range_structure() {
    let ty = PhpType::parse("int<0, 100>");
    match ty.kind() {
        TypeKind::IntRange(min, max) => {
            assert_eq!(min, "0");
            assert_eq!(max, "100");
        }
        other => panic!("Expected IntRange, got {other:?}"),
    }
}

#[test]
fn key_of_structure() {
    let ty = PhpType::parse("key-of<T>");
    match ty.kind() {
        TypeKind::KeyOf(inner) => {
            assert_eq!(*inner, PhpType::named(atom("T")));
        }
        other => panic!("Expected KeyOf, got {other:?}"),
    }
}

#[test]
fn value_of_structure() {
    let ty = PhpType::parse("value-of<T>");
    match ty.kind() {
        TypeKind::ValueOf(inner) => {
            assert_eq!(*inner, PhpType::named(atom("T")));
        }
        other => panic!("Expected ValueOf, got {other:?}"),
    }
}

#[test]
fn value_of_shape_dedups_non_adjacent_values() {
    // `value-of<array{a: int, b: string, c: int}>` must collapse the
    // two non-adjacent `int` values into a single union member.
    let ty = PhpType::parse("value-of<array{a: int, b: string, c: int}>");
    // `substitute` short-circuits on an empty map, so pass an unrelated
    // binding to force the `value-of` evaluation path to run.
    let subs = std::collections::HashMap::from([("T".to_string(), PhpType::named(atom("int")))]);
    let evaluated = ty.substitute(&subs);
    match &evaluated.kind() {
        TypeKind::Union(members) => {
            assert_eq!(
                members.len(),
                2,
                "expected int|string (deduped), got {evaluated:?}"
            );
        }
        other => panic!("Expected a 2-member union, got {other:?}"),
    }
}

#[test]
fn literal_int() {
    let ty = PhpType::parse("42");
    assert_eq!(ty, PhpType::literal_int("42"));
}

#[test]
fn literal_string() {
    let ty = PhpType::parse("'foo'");
    assert_eq!(ty, PhpType::literal_string_raw("'foo'"));
}

#[test]
fn widen_scalar_literals_handles_direct_wrapped_and_nested_union_types() {
    assert_eq!(
        PhpType::literal_string_raw("'draft'").widen_scalar_literals(),
        PhpType::string()
    );
    assert_eq!(
        PhpType::literal_int("42").widen_scalar_literals(),
        PhpType::int()
    );
    assert_eq!(
        PhpType::literal_float("1.5").widen_scalar_literals(),
        PhpType::float()
    );

    let nested = PhpType::union(vec![
        PhpType::union(vec![
            PhpType::literal_int("1"),
            PhpType::literal_float("1.5"),
        ]),
        PhpType::literal_int("2"),
    ]);
    assert_eq!(
        nested.widen_scalar_literals(),
        PhpType::union(vec![PhpType::int(), PhpType::float()])
    );

    let nullable = PhpType::nullable(PhpType::literal_string_raw("'draft'"));
    assert_eq!(
        nullable.widen_scalar_literals(),
        PhpType::nullable(PhpType::string())
    );
}

#[test]
fn widen_scalar_literals_does_not_rewrite_structural_payloads() {
    let generic = PhpType::generic("Box", vec![PhpType::literal_string_raw("'draft'")]);
    let shape = PhpType::array_shape(vec![ShapeEntry {
        key: Some("state".to_string()),
        value_type: PhpType::literal_string_raw("'draft'"),
        optional: false,
    }]);
    let callable = PhpType::parse("callable(): 'draft'");

    assert_eq!(generic.widen_scalar_literals(), generic);
    assert_eq!(shape.widen_scalar_literals(), shape);
    assert_eq!(callable.widen_scalar_literals(), callable);

    let union = PhpType::union(vec![
        generic.clone(),
        PhpType::literal_string_raw("'outside'"),
    ]);
    assert_eq!(
        union.widen_scalar_literals(),
        PhpType::union(vec![generic, PhpType::string()])
    );
}

#[test]
fn int_coercible_array_keys_include_numeric_literals_but_not_null() {
    assert!(PhpType::literal_int("1").is_int_coercible_key());
    assert!(PhpType::literal_float("1.5").is_int_coercible_key());
    assert!(PhpType::parse("int<1, 10>").is_int_coercible_key());
    assert!(
        PhpType::union(vec![
            PhpType::literal_int("1"),
            PhpType::literal_float("1.5"),
        ])
        .is_int_coercible_key()
    );
    assert!(!PhpType::null().is_int_coercible_key());
    assert!(!PhpType::nullable(PhpType::int()).is_int_coercible_key());
    assert!(PhpType::named(atom("number")).is_int_coercible_key());
    assert!(!PhpType::named(atom("Number")).is_int_coercible_key());
}

#[test]
fn parse_model_property_generic() {
    let ty = PhpType::parse("model-property<Process>");
    assert_eq!(
        ty,
        PhpType::generic("model-property", vec![PhpType::named(atom("Process"))])
    );
}

#[test]
fn parse_model_property_nested_in_array() {
    let ty = PhpType::parse("array<model-property<Process>, mixed>");
    assert_eq!(
        ty,
        PhpType::generic(
            "array",
            vec![
                PhpType::generic("model-property", vec![PhpType::named(atom("Process"))]),
                PhpType::mixed(),
            ]
        )
    );
}

// ─── extract_value_type tests ───────────────────────────────────────────

#[test]
fn extract_value_type_array_slice() {
    let ty = PhpType::parse("User[]");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(*val, PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_array_slice_scalar_skipped() {
    let ty = PhpType::parse("int[]");
    assert!(ty.extract_value_type(true).is_none());
}

#[test]
fn extract_value_type_array_slice_scalar_not_skipped() {
    let ty = PhpType::parse("int[]");
    let val = ty.extract_value_type(false).unwrap();
    assert_eq!(*val, PhpType::int());
}

#[test]
fn extract_value_type_list() {
    let ty = PhpType::parse("list<User>");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(*val, PhpType::named(atom("User")));
}

#[test]
fn iterable_element_type_positional_shape_unions_values() {
    // Iterating a heterogeneous tuple yields the union of its
    // positional value types.
    let ty = PhpType::parse("array{int, string}");
    let elem = ty.iterable_element_type().unwrap();
    assert_eq!(
        elem,
        PhpType::union(vec![PhpType::int(), PhpType::string()])
    );
}

#[test]
fn iterable_element_type_homogeneous_shape_collapses() {
    // A shape whose entries share a type iterates to that single type,
    // not a redundant union.
    let ty = PhpType::parse("array{User, User}");
    let elem = ty.iterable_element_type().unwrap();
    assert_eq!(elem, PhpType::named(atom("User")));
}

#[test]
fn iterable_element_type_delegates_for_generics() {
    // Non-shape iterables behave exactly like extract_value_type(false).
    let ty = PhpType::parse("list<User>");
    assert_eq!(
        ty.iterable_element_type().unwrap(),
        PhpType::named(atom("User"))
    );
}

#[test]
fn iterable_element_type_joins_every_union_member() {
    let ty = PhpType::union(vec![
        PhpType::parse("list<int>"),
        PhpType::parse("array<string, float>"),
    ]);
    assert_eq!(
        ty.iterable_element_type().unwrap(),
        PhpType::union(vec![PhpType::int(), PhpType::float()])
    );
}

#[test]
fn extract_value_type_array_two_params() {
    let ty = PhpType::parse("array<int, User>");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(*val, PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_collection() {
    let ty = PhpType::parse("Collection<int, User>");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(*val, PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_generator() {
    // Generator<TKey, TValue, TSend, TReturn> — value is 2nd param
    let ty = PhpType::parse("Generator<int, User, mixed, void>");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(*val, PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_generator_single_param() {
    // Single-param Generator<User> — treated as value type
    let ty = PhpType::parse("Generator<User>");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(*val, PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_nullable() {
    let ty = PhpType::parse("?list<User>");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(*val, PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_scalar_returns_none() {
    let ty = PhpType::int();
    assert!(ty.extract_value_type(true).is_none());
}

#[test]
fn extract_value_type_plain_class_returns_none() {
    let ty = PhpType::named(atom("User"));
    assert!(ty.extract_value_type(true).is_none());
}

#[test]
fn extract_value_type_union_with_generic_array() {
    // User|array<User> — the array member carries the element type.
    let ty = PhpType::parse("User|array<User>");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(val, &PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_union_with_array_slice() {
    // string|User[] — the array-slice member carries the element type.
    let ty = PhpType::parse("string|User[]");
    let val = ty.extract_value_type(true).unwrap();
    assert_eq!(val, &PhpType::named(atom("User")));
}

#[test]
fn extract_value_type_union_no_array_member() {
    // string|int — no array-like member, so no value type.
    let ty = PhpType::parse("string|int");
    assert!(ty.extract_value_type(false).is_none());
}

#[test]
fn extract_value_type_union_skips_scalar_element() {
    // User|array<int> — with skip_scalar=true, the int element is skipped.
    let ty = PhpType::parse("User|array<int>");
    assert!(ty.extract_value_type(true).is_none());
}

#[test]
fn extract_value_type_union_includes_scalar_element() {
    // User|array<int> — with skip_scalar=false, the int element is returned.
    let ty = PhpType::parse("User|array<int>");
    let val = ty.extract_value_type(false).unwrap();
    assert_eq!(val, &PhpType::named(atom("int")));
}

#[test]
fn extract_value_type_shape_element_with_class_value_not_skipped() {
    // array<int, array{price: Decimal}> — the shape element carries a
    // non-scalar value, so skip_scalar=true must not discard it.
    let ty = PhpType::parse("array<int, array{price: Decimal}>");
    let val = ty.extract_value_type(true).unwrap();
    assert!(matches!(val.kind(), TypeKind::ArrayShape(_)));
}

#[test]
fn extract_value_type_shape_element_all_scalar_skipped() {
    // array<int, array{count: int}> — every shape value is scalar, so
    // skip_scalar=true skips the element.
    let ty = PhpType::parse("array<int, array{count: int}>");
    assert!(ty.extract_value_type(true).is_none());
}

// ─── extract_key_type tests ─────────────────────────────────────────────

#[test]
fn extract_key_type_two_params() {
    let ty = PhpType::parse("array<string, User>");
    let key = ty.extract_key_type(false).unwrap();
    assert_eq!(*key, PhpType::string());
}

#[test]
fn extract_key_type_scalar_skipped() {
    let ty = PhpType::parse("array<int, User>");
    assert!(ty.extract_key_type(true).is_none());
}

#[test]
fn extract_key_type_single_param_returns_none() {
    let ty = PhpType::parse("list<User>");
    assert!(ty.extract_key_type(false).is_none());
}

#[test]
fn extract_key_type_slice_returns_none() {
    let ty = PhpType::parse("User[]");
    assert!(ty.extract_key_type(false).is_none());
}

#[test]
fn extract_key_type_class_key() {
    let ty = PhpType::parse("array<Request, Response>");
    let key = ty.extract_key_type(true).unwrap();
    assert_eq!(*key, PhpType::named(atom("Request")));
}

#[test]
fn extract_key_type_union_with_keyed_array() {
    // User|array<string, User> — the array member carries the key type.
    let ty = PhpType::parse("User|array<string, User>");
    let key = ty.extract_key_type(false).unwrap();
    assert_eq!(*key, PhpType::named(atom("string")));
}

#[test]
fn extract_key_type_union_no_keyed_member() {
    // string|int — no array-like member, so no key type.
    let ty = PhpType::parse("string|int");
    assert!(ty.extract_key_type(false).is_none());
}

// ─── non_null_type tests ────────────────────────────────────────────────

#[test]
fn non_null_type_nullable() {
    let ty = PhpType::parse("?User");
    let non_null = ty.non_null_type().unwrap();
    assert_eq!(non_null, PhpType::named(atom("User")));
}

#[test]
fn non_null_type_union_with_null() {
    let ty = PhpType::parse("User|null");
    let non_null = ty.non_null_type().unwrap();
    assert_eq!(non_null, PhpType::named(atom("User")));
}

#[test]
fn non_null_type_union_multiple_non_null() {
    let ty = PhpType::parse("User|Admin|null");
    let non_null = ty.non_null_type().unwrap();
    match non_null.kind() {
        TypeKind::Union(members) => {
            assert_eq!(members.len(), 2);
            assert_eq!(members[0], PhpType::named(atom("User")));
            assert_eq!(members[1], PhpType::named(atom("Admin")));
        }
        other => panic!("Expected Union, got {other:?}"),
    }
}

#[test]
fn non_null_type_no_null() {
    let ty = PhpType::named(atom("User"));
    assert!(ty.non_null_type().is_none());
}

#[test]
fn non_null_type_bare_null() {
    let ty = PhpType::null();
    assert!(ty.non_null_type().is_none());
}

// ─── all_members_scalar tests ───────────────────────────────────────────

#[test]
fn all_members_scalar_int() {
    assert!(PhpType::int().all_members_scalar());
}

#[test]
fn all_members_scalar_string_or_null() {
    assert!(PhpType::parse("string|null").all_members_scalar());
}

#[test]
fn all_members_scalar_nullable_int() {
    assert!(PhpType::parse("?int").all_members_scalar());
}

#[test]
fn all_members_scalar_class() {
    assert!(!PhpType::named(atom("User")).all_members_scalar());
}

#[test]
fn all_members_scalar_class_or_null() {
    assert!(!PhpType::parse("User|null").all_members_scalar());
}

#[test]
fn all_members_scalar_mixed_union() {
    assert!(!PhpType::parse("int|User").all_members_scalar());
}

// ─── intersection_members tests ─────────────────────────────────────────

#[test]
fn intersection_members_of_intersection() {
    let ty = PhpType::parse("Countable&Traversable");
    let members = ty.intersection_members();
    assert_eq!(members.len(), 2);
}

#[test]
fn intersection_members_of_non_intersection() {
    let ty = PhpType::named(atom("User"));
    let members = ty.intersection_members();
    assert_eq!(members.len(), 1);
    assert_eq!(*members[0], PhpType::named(atom("User")));
}

// ─── resolve_names tests ────────────────────────────────────────────────

#[test]
fn resolve_names_simple_class() {
    let ty = PhpType::named(atom("User"));
    let resolved = ty.resolve_names(&|name| format!("App\\Models\\{}", name));
    assert_eq!(resolved.to_string(), "App\\Models\\User");
}

#[test]
fn resolve_names_scalar_untouched() {
    let ty = PhpType::int();
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "int");
}

#[test]
fn resolve_names_union() {
    let ty = PhpType::parse("User|null");
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "App\\User|null");
}

#[test]
fn resolve_names_generic() {
    let ty = PhpType::parse("Collection<int, User>");
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "App\\Collection<int, App\\User>");
}

#[test]
fn resolve_names_nullable() {
    let ty = PhpType::parse("?User");
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "?App\\User");
}

#[test]
fn resolve_names_array_shape() {
    let ty = PhpType::parse("array{name: string, user: User}");
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "array{name: string, user: App\\User}");
}

#[test]
fn resolve_names_callable() {
    let ty = PhpType::parse("callable(User): Response");
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "callable(App\\User): App\\Response");
}

#[test]
fn resolve_names_keyword_types_untouched() {
    // All of these should pass through without calling the resolver.
    for kw in &[
        "self",
        "static",
        "parent",
        "$this",
        "mixed",
        "void",
        "never",
        "class-string",
        "key-of",
        "value-of",
        "callable",
        "iterable",
        "positive-int",
        "non-empty-string",
        "array-key",
    ] {
        let ty = PhpType::named(atom(kw));
        let resolved = ty.resolve_names(&|name| panic!("should not resolve {}", name));
        assert_eq!(resolved.to_string(), *kw);
    }
}

#[test]
fn resolve_names_class_string_inner() {
    let ty = PhpType::parse("class-string<User>");
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "class-string<App\\User>");
}

#[test]
fn resolve_names_intersection() {
    let ty = PhpType::parse("Countable&Traversable");
    let resolved = ty.resolve_names(&|name| format!("App\\{}", name));
    assert_eq!(resolved.to_string(), "App\\Countable&App\\Traversable");
}

// ─── shorten tests ──────────────────────────────────────────────────────

#[test]
fn shorten_plain_class() {
    let ty = PhpType::named(atom("App\\Models\\User"));
    assert_eq!(ty.shorten().to_string(), "User");
}

#[test]
fn shorten_already_short() {
    let ty = PhpType::named(atom("User"));
    assert_eq!(ty.shorten().to_string(), "User");
}

#[test]
fn shorten_scalar() {
    let ty = PhpType::string();
    assert_eq!(ty.shorten().to_string(), "string");
}

#[test]
fn shorten_union() {
    let ty = PhpType::parse("App\\Models\\User|null");
    assert_eq!(ty.shorten().to_string(), "User|null");
}

#[test]
fn shorten_generic() {
    let ty = PhpType::parse("array<int, App\\Models\\User>");
    assert_eq!(ty.shorten().to_string(), "array<int, User>");
}

#[test]
fn shorten_nullable() {
    let ty = PhpType::parse("?App\\Models\\User");
    assert_eq!(ty.shorten().to_string(), "?User");
}

#[test]
fn shorten_callable() {
    let ty = PhpType::parse("callable(App\\Models\\User): App\\Http\\Response");
    assert_eq!(ty.shorten().to_string(), "callable(User): Response");
}

#[test]
fn shorten_class_string() {
    let ty = PhpType::parse("class-string<App\\Models\\User>");
    assert_eq!(ty.shorten().to_string(), "class-string<User>");
}

#[test]
fn shorten_intersection() {
    let ty = PhpType::parse("App\\Contracts\\Countable&App\\Contracts\\Traversable");
    assert_eq!(ty.shorten().to_string(), "Countable&Traversable");
}

#[test]
fn shorten_array_shape() {
    let ty = PhpType::parse("array{name: string, user: App\\Models\\User}");
    assert_eq!(ty.shorten().to_string(), "array{name: string, user: User}");
}

// ─── is_scalar tests ────────────────────────────────────────────────────

#[test]
fn is_scalar_keywords() {
    assert!(PhpType::int().is_scalar());
    assert!(PhpType::string().is_scalar());
    assert!(PhpType::bool().is_scalar());
    assert!(PhpType::float().is_scalar());
    assert!(!PhpType::mixed().is_scalar());
    assert!(PhpType::void().is_scalar());
    assert!(PhpType::null().is_scalar());
    assert!(PhpType::array().is_scalar());
    assert!(PhpType::callable().is_scalar());
    assert!(PhpType::iterable().is_scalar());
}

#[test]
fn is_scalar_class_is_not() {
    assert!(!PhpType::named(atom("User")).is_scalar());
    assert!(!PhpType::named(atom("App\\Models\\User")).is_scalar());
}

#[test]
fn is_scalar_generic_array() {
    assert!(PhpType::parse("array<int, string>").is_scalar());
}

#[test]
fn is_scalar_generic_class() {
    assert!(!PhpType::parse("Collection<int, User>").is_scalar());
}

#[test]
fn is_scalar_nullable_scalar() {
    assert!(PhpType::parse("?int").is_scalar());
}

#[test]
fn is_scalar_nullable_class() {
    assert!(!PhpType::parse("?User").is_scalar());
}

// ─── is_array_like tests ────────────────────────────────────────────────

#[test]
fn is_array_like_named() {
    assert!(PhpType::array().is_array_like());
    assert!(PhpType::named(atom("list")).is_array_like());
    assert!(PhpType::iterable().is_array_like());
    assert!(PhpType::named(atom("non-empty-array")).is_array_like());
    assert!(PhpType::named(atom("non-empty-list")).is_array_like());
}

#[test]
fn is_array_like_generic() {
    assert!(PhpType::parse("array<int, string>").is_array_like());
    assert!(PhpType::parse("list<User>").is_array_like());
    assert!(PhpType::parse("non-empty-array<string, int>").is_array_like());
}

#[test]
fn is_array_like_slice() {
    assert!(PhpType::parse("User[]").is_array_like());
    assert!(PhpType::parse("int[]").is_array_like());
}

#[test]
fn is_array_like_shape() {
    assert!(PhpType::parse("array{name: string}").is_array_like());
}

#[test]
fn is_array_like_nullable() {
    assert!(PhpType::parse("?array").is_array_like());
    assert!(PhpType::parse("?list<User>").is_array_like());
}

#[test]
fn is_array_like_non_array() {
    assert!(!PhpType::string().is_array_like());
    assert!(!PhpType::int().is_array_like());
    assert!(!PhpType::named(atom("User")).is_array_like());
    assert!(!PhpType::null().is_array_like());
    assert!(!PhpType::parse("Collection<int, User>").is_array_like());
}

// ─── base_name tests ────────────────────────────────────────────────────

#[test]
fn base_name_simple_class() {
    assert_eq!(
        PhpType::named(atom("App\\Models\\User")).base_name(),
        Some("App\\Models\\User")
    );
}

#[test]
fn base_name_strips_leading_backslash() {
    assert_eq!(
        PhpType::named(atom("\\App\\Models\\User")).base_name(),
        Some("App\\Models\\User")
    );
}

#[test]
fn base_name_generic_strips_leading_backslash() {
    assert_eq!(
        PhpType::generic("\\Collection", vec![PhpType::named(atom("User"))]).base_name(),
        Some("Collection")
    );
}

#[test]
fn base_name_nullable_strips_leading_backslash() {
    assert_eq!(
        PhpType::nullable(PhpType::named(atom("\\User"))).base_name(),
        Some("User")
    );
}

#[test]
fn base_name_generic_class() {
    assert_eq!(
        PhpType::parse("Collection<int, User>").base_name(),
        Some("Collection")
    );
}

#[test]
fn base_name_scalar_returns_none() {
    assert_eq!(PhpType::int().base_name(), None);
}

#[test]
fn base_name_nullable_class() {
    assert_eq!(PhpType::parse("?User").base_name(), Some("User"));
}

#[test]
fn base_name_union_returns_none() {
    assert_eq!(PhpType::parse("User|null").base_name(), None);
}

// ─── union_members tests ────────────────────────────────────────────────

#[test]
fn union_members_of_union() {
    let ty = PhpType::parse("int|string|null");
    let members = ty.union_members();
    assert_eq!(members.len(), 3);
}

#[test]
fn union_members_of_non_union() {
    let ty = PhpType::named(atom("User"));
    let members = ty.union_members();
    assert_eq!(members.len(), 1);
    assert_eq!(*members[0], PhpType::named(atom("User")));
}

// ─── equivalent tests ───────────────────────────────────────────────────

#[test]
fn equivalent_identical() {
    let a = PhpType::named(atom("User"));
    let b = PhpType::named(atom("User"));
    assert!(a.equivalent(&b));
}

#[test]
fn equivalent_fqn_vs_short() {
    let a = PhpType::named(atom("App\\Models\\User"));
    let b = PhpType::named(atom("User"));
    assert!(a.equivalent(&b));
}

#[test]
fn equivalent_nullable() {
    let a = PhpType::parse("?App\\Models\\User");
    let b = PhpType::parse("?User");
    assert!(a.equivalent(&b));
}

#[test]
fn equivalent_union_reordered() {
    let a = PhpType::parse("App\\Models\\User|null");
    let b = PhpType::parse("null|User");
    assert!(a.equivalent(&b));
}

#[test]
fn equivalent_generic() {
    let a = PhpType::parse("array<int, App\\Models\\User>");
    let b = PhpType::parse("array<int, User>");
    assert!(a.equivalent(&b));
}

#[test]
fn equivalent_nullable_vs_union_with_null() {
    // `?string` is semantically identical to `string|null`
    let a = PhpType::parse("?string");
    let b = PhpType::parse("string|null");
    assert!(a.equivalent(&b));
    assert!(b.equivalent(&a));
}

#[test]
fn equivalent_nullable_vs_null_first_union() {
    // `?callable` is semantically identical to `null|callable`
    let a = PhpType::parse("?callable");
    let b = PhpType::parse("null|callable");
    assert!(a.equivalent(&b));
}

#[test]
fn equivalent_nullable_vs_three_member_union_not_equal() {
    // `?Foo` is NOT equivalent to `Foo|Bar|null` (different arity)
    let a = PhpType::parse("?Foo");
    let b = PhpType::parse("Foo|Bar|null");
    assert!(!a.equivalent(&b));
}

#[test]
fn equivalent_different_types() {
    let a = PhpType::named(atom("User"));
    let b = PhpType::named(atom("Post"));
    assert!(!a.equivalent(&b));
}

// ── replace_self ────────────────────────────────────────────

#[test]
fn replace_self_named() {
    let ty = PhpType::parse("self");
    assert_eq!(ty.replace_self("App\\User").to_string(), "App\\User");
}

#[test]
fn replace_self_static() {
    let ty = PhpType::parse("static");
    assert_eq!(
        ty.replace_self("App\\User").to_string(),
        "static(App\\User)"
    );
}

#[test]
fn replace_self_this() {
    let ty = PhpType::parse("$this");
    assert_eq!(ty.replace_self("App\\User").to_string(), "$this(App\\User)");
}

#[test]
fn replace_self_in_union() {
    let ty = PhpType::parse("self|null");
    let replaced = ty.replace_self("App\\User");
    assert_eq!(replaced.to_string(), "App\\User|null");
}

#[test]
fn replace_self_in_generic() {
    let ty = PhpType::parse("Collection<int, static>");
    let replaced = ty.replace_self("App\\User");
    assert_eq!(replaced.to_string(), "Collection<int, static(App\\User)>");
}

#[test]
fn replace_self_no_keywords_unchanged() {
    let ty = PhpType::parse("string");
    assert_eq!(ty.replace_self("App\\User").to_string(), "string");
}

#[test]
fn replace_self_nullable() {
    let ty = PhpType::parse("?self");
    assert_eq!(ty.replace_self("App\\User").to_string(), "?App\\User");
}

#[test]
fn replace_self_class_name_unchanged() {
    let ty = PhpType::parse("Collection<int, User>");
    assert_eq!(
        ty.replace_self("App\\Post").to_string(),
        "Collection<int, User>"
    );
}

#[test]
fn replace_self_intersection() {
    let ty = PhpType::parse("self&JsonSerializable");
    let replaced = ty.replace_self("App\\User");
    assert_eq!(replaced.to_string(), "App\\User&JsonSerializable");
}

// ── replace_self_bound ──────────────────────────────────────

#[test]
fn a_fixed_call_target_collapses_static_and_this() {
    // `App\User::create()` pins the class, so there is no late static
    // binding left to carry.
    for keyword in ["static", "$this", "self"] {
        assert_eq!(
            PhpType::parse(keyword)
                .replace_self_bound("App\\User", None)
                .to_string(),
            "App\\User",
            "`{keyword}` should collapse at a fixed call target"
        );
    }
}

#[test]
fn a_forwarding_call_binds_static_over_the_calling_class() {
    // `parent::create()` in `Child extends User` reads the annotation off
    // `User` but still resolves `static` to `Child`.
    let ty = PhpType::parse("static");
    assert_eq!(
        ty.replace_self_bound("App\\User", Some("App\\Child"))
            .to_string(),
        "static(App\\Child)"
    );
}

#[test]
fn self_stays_invariant_through_a_forwarding_call() {
    // `self` names the declaring class whatever the call forwards.
    let ty = PhpType::parse("self");
    assert_eq!(
        ty.replace_self_bound("App\\User", Some("App\\Child"))
            .to_string(),
        "App\\User"
    );
}

#[test]
fn a_fixed_call_target_collapses_a_bound_from_an_earlier_hop() {
    let ty = PhpType::generic("array", vec![PhpType::static_type(atom("App\\Child"))]);
    assert_eq!(
        ty.replace_self_bound("App\\User", None).to_string(),
        "array<App\\Child>"
    );
}

#[test]
fn a_forwarding_call_reaches_nested_keywords() {
    let ty = PhpType::parse("list<static>|null");
    assert_eq!(
        ty.replace_self_bound("App\\User", Some("App\\Child"))
            .to_string(),
        "list<static(App\\Child)>|null"
    );
}

/// The display form of a bounded late-static type is the only spelling it has,
/// so it has to read back as the same type: dropping the bound would turn a
/// value whose class is known into an unresolved `static` keyword.
#[test]
fn a_bounded_late_static_type_round_trips_through_parse() {
    for ty in [
        PhpType::static_type(atom("App\\Foo")),
        PhpType::this_type(atom("App\\Foo")),
        PhpType::static_type(atom("Foo")),
    ] {
        assert_eq!(PhpType::parse(&ty.to_string()), ty);
    }

    // A bound nested inside a larger type has to survive too: `replace_self_bound`
    // reaches into generic arguments, unions, and callable signatures.
    for nested in [
        "list<static(App\\Foo)>|null",
        "array<string, $this(App\\Foo)>",
        "Closure(static(App\\Foo)): $this(App\\Bar)",
        "static(App\\Foo)|static(App\\Bar)",
    ] {
        assert_eq!(PhpType::parse(nested).to_string(), nested);
    }

    // An unbounded `static` keeps its keyword meaning, and text that merely
    // looks like a call is not mistaken for a bound.
    assert!(!matches!(
        PhpType::parse("static").kind(),
        TypeKind::StaticType(_)
    ));
    for not_a_bound in ["static(int)", "static()", "count($items)", "strlen(...)"] {
        assert!(
            !matches!(
                PhpType::parse(not_a_bound).kind(),
                TypeKind::StaticType(_) | TypeKind::ThisType(_)
            ),
            "{not_a_bound} is not a bounded late-static type"
        );
    }
}

// ── resolve_self_refs ───────────────────────────────────────

#[test]
fn resolve_self_refs_bare() {
    let ty = PhpType::parse("self");
    assert_eq!(
        ty.resolve_self_refs("App\\Cat", None).to_string(),
        "App\\Cat"
    );
}

#[test]
fn resolve_self_refs_in_array_element() {
    // `self[]` inside an array must be resolved to the concrete class.
    // Regression: `resolve_names` treats `self` as a keyword and skips
    // it, so array/generic element `self` was left unresolved.
    let ty = PhpType::parse("self[]");
    let resolved = ty.resolve_self_refs("App\\Cat", None);
    assert_eq!(resolved, PhpType::parse("App\\Cat[]"));
}

#[test]
fn resolve_self_refs_static_in_generic() {
    let ty = PhpType::parse("array<static>");
    let resolved = ty.resolve_self_refs("App\\Cat", None);
    assert_eq!(
        resolved,
        PhpType::generic("array", vec![PhpType::static_type(atom("App\\Cat"))])
    );
}

#[test]
fn resolve_self_refs_parent() {
    let ty = PhpType::parse("parent[]");
    let resolved = ty.resolve_self_refs("App\\Cat", Some("App\\Animal"));
    assert_eq!(resolved, PhpType::parse("App\\Animal[]"));
}

#[test]
fn resolve_self_refs_parent_without_parent_class_unchanged() {
    let ty = PhpType::parse("parent");
    assert_eq!(ty.resolve_self_refs("App\\Cat", None).to_string(), "parent");
}

#[test]
fn resolve_self_refs_leaves_other_classes_alone() {
    let ty = PhpType::parse("Other[]");
    let resolved = ty.resolve_self_refs("App\\Cat", None);
    assert_eq!(resolved, PhpType::parse("Other[]"));
}

// ── extract_class_names (recursive) ─────────────────────────

#[test]
fn extract_class_names_simple() {
    let names = PhpType::parse("User").extract_class_names();
    assert_eq!(names, vec!["User"]);
}

#[test]
fn extract_class_names_scalar() {
    let names = PhpType::parse("int").extract_class_names();
    assert!(names.is_empty());
}

#[test]
fn extract_class_names_union() {
    let names = PhpType::parse("User|Admin|null").extract_class_names();
    assert_eq!(names, vec!["User", "Admin"]);
}

#[test]
fn extract_class_names_generic_recurses() {
    let names = PhpType::parse("Collection<int, User>").extract_class_names();
    assert_eq!(names, vec!["Collection", "User"]);
}

#[test]
fn extract_class_names_callable() {
    let names = PhpType::parse("Closure(User): Admin").extract_class_names();
    assert_eq!(names, vec!["User", "Admin"]);
}

#[test]
fn extract_class_names_nullable() {
    let names = PhpType::parse("?User").extract_class_names();
    assert_eq!(names, vec!["User"]);
}

#[test]
fn extract_class_names_no_duplicates() {
    let names = PhpType::parse("User|User").extract_class_names();
    assert_eq!(names, vec!["User"]);
}

// ── top_level_class_names ───────────────────────────────────

#[test]
fn top_level_class_names_simple() {
    let names = PhpType::parse("User").top_level_class_names();
    assert_eq!(names, vec!["User"]);
}

#[test]
fn top_level_class_names_generic_base_only() {
    let names = PhpType::parse("Collection<int, User>").top_level_class_names();
    assert_eq!(names, vec!["Collection"]);
}

#[test]
fn top_level_class_names_union() {
    let names = PhpType::parse("User|Admin").top_level_class_names();
    assert_eq!(names, vec!["User", "Admin"]);
}

#[test]
fn top_level_class_names_nullable() {
    let names = PhpType::parse("?User").top_level_class_names();
    assert_eq!(names, vec!["User"]);
}

#[test]
fn top_level_class_names_union_with_null() {
    let names = PhpType::parse("User|null").top_level_class_names();
    assert_eq!(names, vec!["User"]);
}

#[test]
fn top_level_class_names_scalar_excluded() {
    let names = PhpType::parse("string|int").top_level_class_names();
    assert!(names.is_empty());
}

#[test]
fn top_level_class_names_mixed_union() {
    let names = PhpType::parse("string|User|int|Admin|null").top_level_class_names();
    assert_eq!(names, vec!["User", "Admin"]);
}

#[test]
fn top_level_class_names_array_of_class() {
    let names = PhpType::parse("User[]").top_level_class_names();
    assert_eq!(names, vec!["User"]);
}

#[test]
fn top_level_class_names_array_shape_excluded() {
    let names = PhpType::parse("array{name: string}").top_level_class_names();
    assert!(names.is_empty());
}

#[test]
fn top_level_class_names_intersection() {
    let names = PhpType::parse("User&JsonSerializable").top_level_class_names();
    assert_eq!(names, vec!["User", "JsonSerializable"]);
}

#[test]
fn top_level_class_names_fqn() {
    let names = PhpType::parse("\\App\\Models\\User").top_level_class_names();
    assert_eq!(names, vec!["\\App\\Models\\User"]);
}

// ── substitute ──────────────────────────────────────────────────

fn make_subs(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, PhpType> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), PhpType::parse(v)))
        .collect()
}

#[test]
fn substitute_named_match() {
    let ty = PhpType::parse("TValue");
    let result = ty.substitute(&make_subs(&[("TValue", "User")]));
    assert_eq!(result.to_string(), "User");
}

#[test]
fn substitute_named_no_match() {
    let ty = PhpType::parse("SomeClass");
    let result = ty.substitute(&make_subs(&[("TValue", "User")]));
    assert_eq!(result.to_string(), "SomeClass");
}

#[test]
fn substitute_generic() {
    let ty = PhpType::parse("Collection<TKey, TValue>");
    let subs = make_subs(&[("TKey", "int"), ("TValue", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "Collection<int, User>");
}

#[test]
fn substitute_generic_base_is_template() {
    let ty = PhpType::parse("TContainer<int>");
    let subs = make_subs(&[("TContainer", "Collection")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "Collection<int>");
}

#[test]
fn substitute_union() {
    let ty = PhpType::parse("TValue|null");
    let subs = make_subs(&[("TValue", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "User|null");
}

#[test]
fn substitute_intersection() {
    let ty = PhpType::parse("TFirst&TSecond");
    let subs = make_subs(&[("TFirst", "Countable"), ("TSecond", "Iterator")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "Countable&Iterator");
}

#[test]
fn substitute_nullable() {
    let ty = PhpType::parse("?TValue");
    let subs = make_subs(&[("TValue", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "?User");
}

#[test]
fn substitute_array_shorthand() {
    let ty = PhpType::parse("TValue[]");
    let subs = make_subs(&[("TValue", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "array<User>");
}

#[test]
fn substitute_array_shape() {
    let ty = PhpType::parse("array{name: TValue, age: int}");
    let subs = make_subs(&[("TValue", "string")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "array{name: string, age: int}");
}

#[test]
fn substitute_object_shape() {
    let ty = PhpType::parse("object{item: TValue}");
    let subs = make_subs(&[("TValue", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "object{item: User}");
}

#[test]
fn substitute_callable() {
    let ty = PhpType::parse("Closure(TParam): TReturn");
    let subs = make_subs(&[("TParam", "int"), ("TReturn", "string")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "Closure(int): string");
}

#[test]
fn substitute_callable_no_return() {
    let ty = PhpType::parse("callable(TParam)");
    let subs = make_subs(&[("TParam", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "callable(User)");
}

#[test]
fn substitute_class_string() {
    let ty = PhpType::parse("class-string<T>");
    let subs = make_subs(&[("T", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "class-string<User>");
}

#[test]
fn substitute_key_of() {
    let ty = PhpType::parse("key-of<T>");
    let subs = make_subs(&[("T", "array<string, int>")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "string");
}

#[test]
fn substitute_value_of() {
    let ty = PhpType::parse("value-of<T>");
    let subs = make_subs(&[("T", "array<string, User>")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "User");
}

#[test]
fn substitute_nested_generic() {
    let ty = PhpType::parse("Collection<int, Promise<TValue>>");
    let subs = make_subs(&[("TValue", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "Collection<int, Promise<User>>");
}

#[test]
fn substitute_empty_subs_unchanged() {
    let ty = PhpType::parse("Collection<int, User>");
    let subs: std::collections::HashMap<String, PhpType> = std::collections::HashMap::new();
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "Collection<int, User>");
}

#[test]
fn substitute_scalar_unchanged() {
    let ty = PhpType::parse("int");
    let subs = make_subs(&[("TValue", "User")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "int");
}

#[test]
fn substitute_literal_unchanged() {
    let ty = PhpType::parse("42");
    let subs = make_subs(&[("42", "User")]);
    // Literal nodes are not substituted (only Named nodes are).
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "42");
}

#[test]
fn substitute_conditional() {
    let ty = PhpType::parse("($x is T ? TTrue : TFalse)");
    let subs = make_subs(&[("T", "string"), ("TTrue", "User"), ("TFalse", "null")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "$x is string ? User : null");
}

#[test]
fn substitute_complex_real_world() {
    // Simulates resolving `Generator<TKey, TValue, TSend, TReturn>`
    // with concrete types.
    let ty = PhpType::parse("Generator<TKey, TValue, TSend, TReturn>");
    let subs = make_subs(&[
        ("TKey", "int"),
        ("TValue", "User"),
        ("TSend", "mixed"),
        ("TReturn", "void"),
    ]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "Generator<int, User, mixed, void>");
}

#[test]
fn substitute_replacement_is_complex_type() {
    // When a template param is replaced with a union type.
    let ty = PhpType::parse("array<int, TValue>");
    let subs = make_subs(&[("TValue", "string|int")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "array<int, string|int>");
}

#[test]
fn substitute_union_flattens_nested() {
    // When a union member is replaced with another union.
    let ty = PhpType::parse("TValue|null");
    let subs = make_subs(&[("TValue", "string|int")]);
    let result = ty.substitute(&subs);
    // Should flatten to a single union, not `(string|int)|null`.
    match result.kind() {
        TypeKind::Union(members) => assert_eq!(members.len(), 3),
        other => panic!("expected Union, got: {other:?}"),
    }
}

#[test]
fn substitute_index_access() {
    let ty = PhpType::parse("T[K]");
    let subs = make_subs(&[("T", "array<string, int>"), ("K", "string")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "int");
}

#[test]
fn substitute_interface_string() {
    let ty = PhpType::parse("interface-string<T>");
    let subs = make_subs(&[("T", "Countable")]);
    let result = ty.substitute(&subs);
    assert_eq!(result.to_string(), "interface-string<Countable>");
}

// ─── callable_param_types tests ─────────────────────────────────────────

#[test]
fn callable_param_types_on_callable() {
    let ty = PhpType::parse("callable(int, string): bool");
    let params = ty.callable_param_types().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].type_hint, PhpType::int());
    assert_eq!(params[1].type_hint, PhpType::string());
}

#[test]
fn callable_param_types_nullable_callable() {
    let ty = PhpType::parse("?Closure(int): void");
    let params = ty.callable_param_types().unwrap();
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].type_hint, PhpType::int());
}

#[test]
fn callable_param_types_union_with_callable() {
    let ty = PhpType::parse("Closure(string, int): void|null");
    let params = ty.callable_param_types().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].type_hint, PhpType::string());
    assert_eq!(params[1].type_hint, PhpType::int());
}

#[test]
fn callable_param_types_non_callable() {
    let ty = PhpType::int();
    assert!(ty.callable_param_types().is_none());
}

// ─── callable_return_type tests ─────────────────────────────────────────

#[test]
fn callable_return_type_with_return() {
    let ty = PhpType::parse("callable(int): User");
    let ret = ty.callable_return_type().unwrap();
    assert_eq!(*ret, PhpType::named(atom("User")));
}

#[test]
fn callable_return_type_without_return() {
    let ty = PhpType::callable_spec("callable", vec![], None);
    assert!(ty.callable_return_type().is_none());
}

#[test]
fn callable_return_type_nullable_callable() {
    let ty = PhpType::parse("?Closure(string): User");
    let ret = ty.callable_return_type().unwrap();
    assert_eq!(*ret, PhpType::named(atom("User")));
}

#[test]
fn callable_return_type_union_with_callable() {
    let ty = PhpType::parse("Closure(int): Response|null");
    let ret = ty.callable_return_type().unwrap();
    assert_eq!(*ret, PhpType::named(atom("Response")));
}

#[test]
fn callable_return_type_non_callable() {
    let ty = PhpType::string();
    assert!(ty.callable_return_type().is_none());
}

// ─── generator_send_type tests ──────────────────────────────────────────

#[test]
fn generator_send_type_full_generator() {
    let ty = PhpType::parse("Generator<int, string, MyClass, void>");
    let send = ty.generator_send_type(false).unwrap();
    assert_eq!(*send, PhpType::named(atom("MyClass")));
}

#[test]
fn generator_send_type_skip_scalar_false_returns_scalar() {
    let ty = PhpType::parse("Generator<int, string, int, void>");
    let send = ty.generator_send_type(false).unwrap();
    assert_eq!(*send, PhpType::int());
}

#[test]
fn generator_send_type_skip_scalar_true_skips_scalar() {
    let ty = PhpType::parse("Generator<int, string, int, void>");
    assert!(ty.generator_send_type(true).is_none());
}

#[test]
fn generator_send_type_skip_scalar_true_keeps_class() {
    let ty = PhpType::parse("Generator<int, string, MyClass, void>");
    let send = ty.generator_send_type(true).unwrap();
    assert_eq!(*send, PhpType::named(atom("MyClass")));
}

#[test]
fn generator_send_type_fewer_than_three_params() {
    let ty = PhpType::parse("Generator<int, string>");
    assert!(ty.generator_send_type(false).is_none());
}

#[test]
fn generator_send_type_non_generator() {
    let ty = PhpType::named(atom("Collection"));
    assert!(ty.generator_send_type(false).is_none());
}

#[test]
fn generator_send_type_nullable_generator() {
    let ty = PhpType::parse("?Generator<int, string, MyClass, void>");
    let send = ty.generator_send_type(false).unwrap();
    assert_eq!(*send, PhpType::named(atom("MyClass")));
}

// ── Subtype checking tests ──────────────────────────────────────────

mod subtype_tests {
    use super::*;

    // ── Reflexivity ─────────────────────────────────────────────────

    #[test]
    fn subtype_reflexive_named() {
        let t = PhpType::int();
        assert!(t.is_subtype_of(&t));
    }

    #[test]
    fn subtype_reflexive_generic() {
        let t = PhpType::parse("array<int, string>");
        assert!(t.is_subtype_of(&t));
    }

    // ── Never and mixed ─────────────────────────────────────────────

    #[test]
    fn never_is_subtype_of_everything() {
        let never = PhpType::never();
        assert!(never.is_subtype_of(&PhpType::int()));
        assert!(never.is_subtype_of(&PhpType::string()));
        assert!(never.is_subtype_of(&PhpType::mixed()));
        assert!(never.is_subtype_of(&PhpType::parse("array<int>")));
    }

    #[test]
    fn everything_is_subtype_of_mixed() {
        let mixed = PhpType::mixed();
        assert!(PhpType::int().is_subtype_of(&mixed));
        assert!(PhpType::string().is_subtype_of(&mixed));
        assert!(PhpType::parse("Foo").is_subtype_of(&mixed));
        assert!(PhpType::parse("array<int>").is_subtype_of(&mixed));
    }

    // ── Bool subtypes ───────────────────────────────────────────────

    #[test]
    fn true_is_subtype_of_bool() {
        assert!(PhpType::true_().is_subtype_of(&PhpType::bool()));
    }

    #[test]
    fn false_is_subtype_of_bool() {
        assert!(PhpType::false_().is_subtype_of(&PhpType::bool()));
    }

    #[test]
    fn bool_is_not_subtype_of_true() {
        assert!(!PhpType::bool().is_subtype_of(&PhpType::true_()));
    }

    // ── Int <: float ────────────────────────────────────────────────

    #[test]
    fn int_is_subtype_of_float() {
        assert!(PhpType::int().is_subtype_of(&PhpType::float()));
    }

    #[test]
    fn float_is_not_subtype_of_int() {
        assert!(!PhpType::float().is_subtype_of(&PhpType::int()));
    }

    // ── Scalar refinements ──────────────────────────────────────────

    #[test]
    fn positive_int_is_subtype_of_int() {
        assert!(PhpType::named(atom("positive-int")).is_subtype_of(&PhpType::int()));
    }

    #[test]
    fn non_empty_string_is_subtype_of_string() {
        assert!(PhpType::named(atom("non-empty-string")).is_subtype_of(&PhpType::string()));
    }

    #[test]
    fn class_string_is_subtype_of_string() {
        assert!(PhpType::named(atom("class-string")).is_subtype_of(&PhpType::string()));
    }

    /// `non-falsy-string` excludes `"0"` on top of the `""` that
    /// `non-empty-string` excludes, so the `non-empty-…` refinements, all of
    /// which are inhabited by `"0"`, stop at `non-empty-string`. The string
    /// kinds that name a PHP symbol carry on: a symbol name can never be
    /// `"0"`.
    #[test]
    fn only_the_string_kinds_that_exclude_zero_are_non_falsy() {
        for name in [
            "non-empty-literal-string",
            "non-empty-lowercase-string",
            "non-empty-uppercase-string",
            "non-empty-string",
        ] {
            let ty = PhpType::named(atom(name));
            assert!(
                ty.is_subtype_of(&PhpType::named(atom("non-empty-string"))),
                "{name} should be a non-empty-string"
            );
            assert!(
                !ty.is_subtype_of(&PhpType::named(atom("non-falsy-string"))),
                "{name} admits \"0\", so it is not a non-falsy-string"
            );
            assert!(
                !ty.is_subtype_of(&PhpType::named(atom("truthy-string"))),
                "{name} admits \"0\", so it is not a truthy-string"
            );
        }

        for name in ["class-string", "callable-string", "enum-string"] {
            assert!(
                PhpType::named(atom(name)).is_subtype_of(&PhpType::named(atom("non-falsy-string"))),
                "{name} names a PHP symbol, which is never \"0\""
            );
        }
    }

    #[test]
    fn list_is_subtype_of_array() {
        assert!(PhpType::named(atom("list")).is_subtype_of(&PhpType::array()));
    }

    #[test]
    fn non_empty_list_is_subtype_of_non_empty_array() {
        assert!(
            PhpType::named(atom("non-empty-list"))
                .is_subtype_of(&PhpType::named(atom("non-empty-array")))
        );
    }

    /// A shape answers the `non-empty-…` and `list` promises from its own
    /// entries: a required key makes it non-empty, and keys running
    /// `0, 1, 2, …` make it a list.
    #[test]
    fn shape_satisfies_the_array_families_its_entries_promise() {
        let is_subtype =
            |sub: &str, sup: &str| PhpType::parse(sub).is_subtype_of(&PhpType::parse(sup));

        assert!(is_subtype("array{name: string}", "non-empty-array"));
        assert!(is_subtype(
            "array{name: string}",
            "non-empty-array<string, string>"
        ));
        assert!(!is_subtype("array{}", "non-empty-array"));
        assert!(!is_subtype("array{}", "non-empty-array<int, string>"));
        // Every entry may be absent, so the shape may be the empty array.
        assert!(!is_subtype("array{name?: string}", "non-empty-array"));

        assert!(is_subtype("array{string, int}", "list<string|int>"));
        assert!(is_subtype("array{0: string, 1: int}", "list"));
        assert!(is_subtype("array{string}", "non-empty-list<string>"));
        assert!(!is_subtype("array{name: string}", "list"));
        // A hole in the sequence is not a list.
        assert!(!is_subtype("array{0: string, 2: int}", "list<string|int>"));
        assert!(!is_subtype("array{0?: string, 1: int}", "list<string|int>"));
    }

    #[test]
    fn array_is_subtype_of_iterable() {
        assert!(PhpType::array().is_subtype_of(&PhpType::iterable()));
    }

    #[test]
    fn closure_is_subtype_of_callable() {
        assert!(PhpType::named(atom("Closure")).is_subtype_of(&PhpType::callable()));
    }

    #[test]
    fn fqn_closure_is_subtype_of_callable() {
        assert!(PhpType::named(atom("\\Closure")).is_subtype_of(&PhpType::callable()));
    }

    #[test]
    fn fqn_closure_is_subtype_of_callable_union_null() {
        let callable_or_null = PhpType::union(vec![PhpType::callable(), PhpType::null()]);
        assert!(PhpType::named(atom("\\Closure")).is_subtype_of(&callable_or_null));
    }

    // ── Scalar / numeric / array-key supertypes ─────────────────────

    #[test]
    fn int_is_subtype_of_scalar() {
        assert!(PhpType::int().is_subtype_of(&PhpType::named(atom("scalar"))));
    }

    #[test]
    fn string_is_subtype_of_array_key() {
        assert!(PhpType::string().is_subtype_of(&PhpType::named(atom("array-key"))));
    }

    #[test]
    fn array_key_is_subtype_of_int_string_union() {
        let int_or_string = PhpType::union(vec![PhpType::int(), PhpType::string()]);
        assert!(PhpType::named(atom("array-key")).is_subtype_of(&int_or_string));
    }

    #[test]
    fn int_string_union_is_subtype_of_array_key() {
        let int_or_string = PhpType::union(vec![PhpType::int(), PhpType::string()]);
        assert!(int_or_string.is_subtype_of(&PhpType::named(atom("array-key"))));
    }

    #[test]
    fn array_key_is_subtype_of_scalar() {
        assert!(PhpType::named(atom("array-key")).is_subtype_of(&PhpType::named(atom("scalar"))));
    }

    #[test]
    fn array_key_is_not_subtype_of_int_alone() {
        assert!(!PhpType::named(atom("array-key")).is_subtype_of(&PhpType::int()));
    }

    #[test]
    fn int_is_subtype_of_numeric() {
        assert!(PhpType::int().is_subtype_of(&PhpType::numeric()));
    }

    // ── Nullable / union subtyping ──────────────────────────────────

    #[test]
    fn null_is_subtype_of_nullable() {
        assert!(PhpType::null().is_subtype_of(&PhpType::parse("?string")));
    }

    #[test]
    fn string_is_subtype_of_nullable_string() {
        assert!(PhpType::string().is_subtype_of(&PhpType::parse("?string")));
    }

    #[test]
    fn nullable_is_not_subtype_of_non_nullable() {
        assert!(!PhpType::parse("?string").is_subtype_of(&PhpType::string()));
    }

    #[test]
    fn union_member_is_subtype_of_union() {
        assert!(PhpType::int().is_subtype_of(&PhpType::parse("int|string")));
    }

    #[test]
    fn union_is_subtype_when_all_members_are() {
        assert!(PhpType::parse("int|float").is_subtype_of(&PhpType::float()));
    }

    #[test]
    fn union_is_not_subtype_when_member_is_not() {
        assert!(!PhpType::parse("int|string").is_subtype_of(&PhpType::int()));
    }

    // ── Intersection subtyping ──────────────────────────────────────

    #[test]
    fn intersection_is_subtype_when_any_member_is() {
        // Foo & Bar <: Foo
        let inter = PhpType::intersection(vec![
            PhpType::named(atom("Foo")),
            PhpType::named(atom("Bar")),
        ]);
        assert!(inter.is_subtype_of(&PhpType::named(atom("Foo"))));
    }

    #[test]
    fn subtype_of_intersection_requires_all() {
        // Foo <: Foo & Bar — false (Foo is not necessarily a Bar)
        let inter = PhpType::intersection(vec![
            PhpType::named(atom("Foo")),
            PhpType::named(atom("Bar")),
        ]);
        assert!(!PhpType::named(atom("Foo")).is_subtype_of(&inter));
    }

    // ── Array / generic subtyping ───────────────────────────────────

    #[test]
    fn array_slice_is_subtype_of_array() {
        assert!(PhpType::parse("string[]").is_subtype_of(&PhpType::array()));
    }

    #[test]
    fn array_slice_is_not_subtype_of_object() {
        assert!(!PhpType::parse("string[]").is_subtype_of(&PhpType::object()));
    }

    #[test]
    fn array_shape_is_subtype_of_array() {
        assert!(PhpType::parse("array{name: string}").is_subtype_of(&PhpType::array()));
    }

    #[test]
    fn array_shape_is_subtype_of_generic_array_string_mixed() {
        assert!(
            PhpType::parse("array{id: int, refunded_amount: string}")
                .is_subtype_of(&PhpType::parse("array<string, mixed>"))
        );
    }

    #[test]
    fn array_shape_is_subtype_of_generic_array_string_scalar() {
        assert!(
            PhpType::parse("array{name: string, age: int}")
                .is_subtype_of(&PhpType::parse("array<string, string|int>"))
        );
    }

    #[test]
    fn array_shape_not_subtype_of_generic_array_wrong_value() {
        // Shape has an int value but supertype requires string values.
        assert!(
            !PhpType::parse("array{name: string, age: int}")
                .is_subtype_of(&PhpType::parse("array<string, string>"))
        );
    }

    #[test]
    fn array_shape_is_subtype_of_generic_array_single_param() {
        // array<mixed> — only value type checked.
        assert!(
            PhpType::parse("array{name: string, count: int}")
                .is_subtype_of(&PhpType::parse("array<mixed>"))
        );
    }

    #[test]
    fn array_shape_is_subtype_of_array_slice() {
        // array{name: string, label: string} <: string[]
        assert!(
            PhpType::parse("array{name: string, label: string}")
                .is_subtype_of(&PhpType::parse("string[]"))
        );
    }

    #[test]
    fn array_shape_not_subtype_of_array_slice_wrong_value() {
        assert!(
            !PhpType::parse("array{name: string, age: int}")
                .is_subtype_of(&PhpType::parse("string[]"))
        );
    }

    #[test]
    fn array_shape_with_int_keys_subtype_of_array_int_mixed() {
        // Positional entries have int keys.
        assert!(
            PhpType::parse("array{string, int}")
                .is_subtype_of(&PhpType::parse("array<int, mixed>"))
        );
    }

    #[test]
    fn generic_array_is_subtype_of_array() {
        assert!(PhpType::parse("array<int, string>").is_subtype_of(&PhpType::array()));
    }

    #[test]
    fn generic_array_covariance() {
        assert!(
            PhpType::parse("array<int, string>")
                .is_subtype_of(&PhpType::parse("array<int, string>"))
        );
    }

    #[test]
    fn generic_list_is_subtype_of_generic_array() {
        assert!(PhpType::parse("list<string>").is_subtype_of(&PhpType::parse("array<string>")));
    }

    #[test]
    fn array_like_generics_compare_across_arities() {
        // `list<V>` is `array<int, V>` spelled at a different arity, so the
        // key/value pair has to be compared rather than the argument lists.
        assert!(
            PhpType::parse("list<string>").is_subtype_of(&PhpType::parse("array<int, string>"))
        );
        assert!(
            PhpType::parse("list<array{item: string, qty: int}>")
                .is_subtype_of(&PhpType::parse("array<int, array<string, mixed>>"))
        );
        // `array<V>` keys on `array-key`, which a `string` key cannot accept.
        assert!(PhpType::parse("array<string, int>").is_subtype_of(&PhpType::parse("array<int>")));
        assert!(!PhpType::parse("array<int>").is_subtype_of(&PhpType::parse("array<string, int>")));
        // A one-argument `iterable` says nothing about its key type.
        assert!(
            PhpType::parse("array<int, string>").is_subtype_of(&PhpType::parse("iterable<string>"))
        );
    }

    #[test]
    fn generic_array_is_not_subtype_of_list_across_arities() {
        // A `list` supertype demands sequential integer keys.
        assert!(
            !PhpType::parse("array<int, string>").is_subtype_of(&PhpType::parse("list<string>"))
        );
    }

    #[test]
    fn array_slice_covariance() {
        // int[] <: int[] — reflexive
        assert!(PhpType::parse("int[]").is_subtype_of(&PhpType::parse("int[]")));
    }

    // ── class-string subtyping ──────────────────────────────────────

    #[test]
    fn class_string_generic_is_subtype_of_bare_class_string() {
        assert!(
            PhpType::parse("class-string<User>").is_subtype_of(&PhpType::parse("class-string"))
        );
    }

    #[test]
    fn class_string_generic_is_subtype_of_string() {
        assert!(PhpType::parse("class-string<User>").is_subtype_of(&PhpType::string()));
    }

    // ── Callable subtyping ──────────────────────────────────────────

    #[test]
    fn callable_is_subtype_of_named_callable() {
        assert!(PhpType::parse("callable(int): string").is_subtype_of(&PhpType::callable()));
    }

    #[test]
    fn callable_covariant_return() {
        // callable(): int <: callable(): float (int <: float)
        assert!(
            PhpType::parse("callable(): int").is_subtype_of(&PhpType::parse("callable(): float"))
        );
    }

    // ── Literal subtyping ───────────────────────────────────────────

    #[test]
    fn literal_int_is_subtype_of_int() {
        assert!(PhpType::literal_int("42").is_subtype_of(&PhpType::int()));
    }

    #[test]
    fn literal_string_is_subtype_of_string() {
        assert!(PhpType::literal_string_raw("'hello'").is_subtype_of(&PhpType::string()));
    }

    #[test]
    fn literal_int_is_subtype_of_float() {
        assert!(PhpType::literal_int("42").is_subtype_of(&PhpType::float()));
    }

    #[test]
    fn literal_numeric_string_is_subtype_of_numeric_string() {
        assert!(
            PhpType::literal_string_raw("'0.00'").is_subtype_of(&PhpType::parse("numeric-string"))
        );
    }

    #[test]
    fn literal_integer_string_is_subtype_of_numeric_string() {
        assert!(
            PhpType::literal_string_raw("'42'").is_subtype_of(&PhpType::parse("numeric-string"))
        );
    }

    #[test]
    fn literal_non_numeric_string_is_not_subtype_of_numeric_string() {
        assert!(
            !PhpType::literal_string_raw("'hello'")
                .is_subtype_of(&PhpType::parse("numeric-string"))
        );
    }

    #[test]
    fn literal_empty_string_is_not_subtype_of_non_empty_string() {
        assert!(
            !PhpType::literal_string_raw("''").is_subtype_of(&PhpType::parse("non-empty-string"))
        );
    }

    #[test]
    fn literal_non_empty_string_is_subtype_of_non_empty_string() {
        assert!(
            PhpType::literal_string_raw("'foo'").is_subtype_of(&PhpType::parse("non-empty-string"))
        );
    }

    #[test]
    fn literal_string_is_subtype_of_truthy_string() {
        assert!(
            PhpType::literal_string_raw("'foo'").is_subtype_of(&PhpType::parse("truthy-string"))
        );
    }

    #[test]
    fn literal_zero_string_is_not_subtype_of_truthy_string() {
        assert!(
            !PhpType::literal_string_raw("'0'").is_subtype_of(&PhpType::parse("truthy-string"))
        );
    }

    #[test]
    fn literal_empty_string_is_not_subtype_of_truthy_string() {
        assert!(!PhpType::literal_string_raw("''").is_subtype_of(&PhpType::parse("truthy-string")));
    }

    #[test]
    fn literal_negative_numeric_string_is_subtype_of_numeric_string() {
        assert!(
            PhpType::literal_string_raw("'-3.14'").is_subtype_of(&PhpType::parse("numeric-string"))
        );
    }

    #[test]
    fn literal_source_syntax_string_is_not_subtype_of_numeric_string() {
        // A runtime string is numeric per PHP's is_numeric, which does
        // not accept underscores or hex/binary/octal prefixes even
        // though they are valid in PHP source literals.
        for raw in ["'1_000'", "'0xFF'", "'0b101'", "'0o17'"] {
            assert!(
                !PhpType::literal_string_raw(raw).is_subtype_of(&PhpType::parse("numeric-string")),
                "{raw} should not be a numeric-string"
            );
        }
    }

    #[test]
    fn literal_numeric_string_follows_php_8_grammar() {
        for raw in [
            "' 42 '",
            "'+.5'",
            "'5.'",
            "'-1.5e+2'",
            "'1e309'",
            "'\t7\r\n'",
        ] {
            assert!(
                PhpType::literal_string_raw(raw).is_subtype_of(&PhpType::parse("numeric-string")),
                "{raw} should be a PHP numeric-string"
            );
        }

        for raw in ["'INF'", "'-inf'", "'NaN'", "'.'", "'1e'", "'+'"] {
            assert!(
                !PhpType::literal_string_raw(raw).is_subtype_of(&PhpType::parse("numeric-string")),
                "{raw} should not be a PHP numeric-string"
            );
        }
    }

    // ── IntRange subtyping ──────────────────────────────────────────

    #[test]
    fn int_range_is_subtype_of_int() {
        assert!(PhpType::int_range("0", "100").is_subtype_of(&PhpType::int()));
    }

    #[test]
    fn integer_literal_is_subtype_of_int_range() {
        assert!(PhpType::literal_int("10000").is_subtype_of(&PhpType::int_range("0", "max")));
        assert!(PhpType::literal_int("1").is_subtype_of(&PhpType::int_range("1", "59")));
        assert!(!PhpType::literal_int("0").is_subtype_of(&PhpType::int_range("1", "59")));
    }

    #[test]
    fn float_literal_is_not_subtype_of_int_range() {
        assert!(!PhpType::literal_float("1.0").is_subtype_of(&PhpType::int_range("0", "max")));
    }

    // ── Integer literal <: named refined-int ─────────────────────

    #[test]
    fn positive_integer_literal_is_subtype_of_non_negative_int() {
        assert!(PhpType::literal_int("1").is_subtype_of(&PhpType::named(atom("non-negative-int"))));
    }

    #[test]
    fn positive_integer_literal_is_subtype_of_positive_int() {
        assert!(PhpType::literal_int("1").is_subtype_of(&PhpType::named(atom("positive-int"))));
    }

    #[test]
    fn zero_literal_is_not_subtype_of_positive_int() {
        assert!(!PhpType::literal_int("0").is_subtype_of(&PhpType::named(atom("positive-int"))));
    }

    #[test]
    fn zero_literal_is_subtype_of_non_negative_int() {
        assert!(PhpType::literal_int("0").is_subtype_of(&PhpType::named(atom("non-negative-int"))));
    }

    #[test]
    fn zero_literal_is_not_subtype_of_non_zero_int() {
        assert!(!PhpType::literal_int("0").is_subtype_of(&PhpType::named(atom("non-zero-int"))));
    }

    #[test]
    fn positive_integer_literal_is_subtype_of_non_zero_int() {
        assert!(PhpType::literal_int("1").is_subtype_of(&PhpType::named(atom("non-zero-int"))));
    }

    #[test]
    fn negative_integer_literal_is_subtype_of_negative_int() {
        assert!(PhpType::literal_int("-1").is_subtype_of(&PhpType::named(atom("negative-int"))));
    }

    #[test]
    fn positive_integer_literal_is_not_subtype_of_negative_int() {
        assert!(!PhpType::literal_int("1").is_subtype_of(&PhpType::named(atom("negative-int"))));
    }

    #[test]
    fn zero_literal_is_subtype_of_non_positive_int() {
        assert!(PhpType::literal_int("0").is_subtype_of(&PhpType::named(atom("non-positive-int"))));
    }

    // ── IntRange <: refined-int ──────────────────────────────────

    #[test]
    fn int_range_0_max_is_subtype_of_non_negative_int() {
        assert!(
            PhpType::int_range("0", "max").is_subtype_of(&PhpType::named(atom("non-negative-int")))
        );
    }

    #[test]
    fn int_range_1_max_is_subtype_of_positive_int() {
        assert!(
            PhpType::int_range("1", "max").is_subtype_of(&PhpType::named(atom("positive-int")))
        );
    }

    #[test]
    fn int_range_1_max_is_subtype_of_non_negative_int() {
        // positive-int range is a subset of non-negative-int range
        assert!(
            PhpType::int_range("1", "max").is_subtype_of(&PhpType::named(atom("non-negative-int")))
        );
    }

    #[test]
    fn int_range_min_neg1_is_subtype_of_negative_int() {
        assert!(
            PhpType::int_range("min", "-1").is_subtype_of(&PhpType::named(atom("negative-int")))
        );
    }

    #[test]
    fn int_range_min_0_is_subtype_of_non_positive_int() {
        assert!(
            PhpType::int_range("min", "0").is_subtype_of(&PhpType::named(atom("non-positive-int")))
        );
    }

    #[test]
    fn int_range_0_100_is_subtype_of_non_negative_int() {
        assert!(
            PhpType::int_range("0", "100").is_subtype_of(&PhpType::named(atom("non-negative-int")))
        );
    }

    #[test]
    fn int_range_neg1_max_is_not_subtype_of_non_negative_int() {
        assert!(
            !PhpType::int_range("-1", "max")
                .is_subtype_of(&PhpType::named(atom("non-negative-int")))
        );
    }

    #[test]
    fn int_range_0_max_is_not_subtype_of_positive_int() {
        // 0..max includes 0 which is not positive
        assert!(
            !PhpType::int_range("0", "max").is_subtype_of(&PhpType::named(atom("positive-int")))
        );
    }

    // ── refined-int <: IntRange ─────────────────────────────────────

    #[test]
    fn non_negative_int_is_subtype_of_int_range_0_max() {
        assert!(
            PhpType::named(atom("non-negative-int")).is_subtype_of(&PhpType::int_range("0", "max"))
        );
    }

    #[test]
    fn positive_int_is_subtype_of_int_range_0_max() {
        assert!(
            PhpType::named(atom("positive-int")).is_subtype_of(&PhpType::int_range("0", "max"))
        );
    }

    #[test]
    fn negative_int_is_subtype_of_int_range_min_neg1() {
        assert!(
            PhpType::named(atom("negative-int")).is_subtype_of(&PhpType::int_range("min", "-1"))
        );
    }

    #[test]
    fn positive_int_is_not_subtype_of_int_range_min_neg1() {
        assert!(
            !PhpType::named(atom("positive-int")).is_subtype_of(&PhpType::int_range("min", "-1"))
        );
    }

    // ── IntRange <: IntRange ────────────────────────────────────────

    #[test]
    fn int_range_1_50_is_subtype_of_0_100() {
        assert!(PhpType::int_range("1", "50").is_subtype_of(&PhpType::int_range("0", "100")));
    }

    #[test]
    fn int_range_0_100_is_not_subtype_of_1_50() {
        assert!(!PhpType::int_range("0", "100").is_subtype_of(&PhpType::int_range("1", "50")));
    }

    #[test]
    fn int_range_0_max_is_subtype_of_0_max() {
        assert!(PhpType::int_range("0", "max").is_subtype_of(&PhpType::int_range("0", "max")));
    }

    // ── refined-int <: refined-int ──────────────────────────────────

    #[test]
    fn positive_int_is_subtype_of_non_negative_int() {
        assert!(
            PhpType::named(atom("positive-int"))
                .is_subtype_of(&PhpType::named(atom("non-negative-int")))
        );
    }

    #[test]
    fn negative_int_is_subtype_of_non_positive_int() {
        assert!(
            PhpType::named(atom("negative-int"))
                .is_subtype_of(&PhpType::named(atom("non-positive-int")))
        );
    }

    #[test]
    fn non_negative_int_is_not_subtype_of_positive_int() {
        // non-negative includes 0, positive doesn't
        assert!(
            !PhpType::named(atom("non-negative-int"))
                .is_subtype_of(&PhpType::named(atom("positive-int")))
        );
    }

    #[test]
    fn positive_int_is_not_subtype_of_negative_int() {
        assert!(
            !PhpType::named(atom("positive-int"))
                .is_subtype_of(&PhpType::named(atom("negative-int")))
        );
    }

    // ── non-zero-int subtyping ──────────────────────────────────────

    #[test]
    fn positive_int_is_subtype_of_non_zero_int() {
        assert!(
            PhpType::named(atom("positive-int"))
                .is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn negative_int_is_subtype_of_non_zero_int() {
        assert!(
            PhpType::named(atom("negative-int"))
                .is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn non_negative_int_is_not_subtype_of_non_zero_int() {
        // non-negative includes 0
        assert!(
            !PhpType::named(atom("non-negative-int"))
                .is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn non_positive_int_is_not_subtype_of_non_zero_int() {
        // non-positive includes 0
        assert!(
            !PhpType::named(atom("non-positive-int"))
                .is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn int_range_1_max_is_subtype_of_non_zero_int() {
        assert!(
            PhpType::int_range("1", "max").is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn int_range_min_neg1_is_subtype_of_non_zero_int() {
        assert!(
            PhpType::int_range("min", "-1").is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn int_range_0_max_is_not_subtype_of_non_zero_int() {
        // includes 0
        assert!(
            !PhpType::int_range("0", "max").is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn int_range_neg5_5_is_not_subtype_of_non_zero_int() {
        // includes 0
        assert!(
            !PhpType::int_range("-5", "5").is_subtype_of(&PhpType::named(atom("non-zero-int")))
        );
    }

    #[test]
    fn php_integer_literal_syntaxes_compare_by_value() {
        assert!(PhpType::literal_int("0x10").is_subtype_of(&PhpType::literal_int("16")));
        assert!(PhpType::literal_int("0b1010").is_subtype_of(&PhpType::literal_int("10")));
        assert!(PhpType::literal_int("0o10").is_subtype_of(&PhpType::literal_int("8")));
        assert!(PhpType::literal_int("1_000").is_subtype_of(&PhpType::literal_int("1000")));
    }

    #[test]
    fn php_float_literal_syntaxes_compare_by_value() {
        assert!(PhpType::literal_float("1.0").is_subtype_of(&PhpType::literal_float("1.00")));
        assert!(PhpType::literal_float("1e3").is_subtype_of(&PhpType::literal_float("1000.0")));
        assert!(PhpType::literal_float("1_2.5e2").is_subtype_of(&PhpType::literal_float("1250.0")));
    }

    // ── Unrelated types ─────────────────────────────────────────────

    #[test]
    fn string_is_not_subtype_of_int() {
        assert!(!PhpType::string().is_subtype_of(&PhpType::int()));
    }

    #[test]
    fn unrelated_classes_are_not_subtypes() {
        assert!(!PhpType::named(atom("Cat")).is_subtype_of(&PhpType::named(atom("Dog"))));
    }

    // ── Aliases ─────────────────────────────────────────────────────

    #[test]
    fn integer_alias_subtype_of_int() {
        assert!(PhpType::named(atom("integer")).is_subtype_of(&PhpType::int()));
    }

    #[test]
    fn boolean_alias_subtype_of_bool() {
        assert!(PhpType::named(atom("boolean")).is_subtype_of(&PhpType::bool()));
    }

    // ── object shape <: object ──────────────────────────────────────

    #[test]
    fn object_shape_is_subtype_of_object() {
        assert!(PhpType::parse("object{name: string}").is_subtype_of(&PhpType::object()));
    }
}

// ── Simplification tests ────────────────────────────────────────────

mod simplification_tests {
    use super::*;

    #[test]
    fn dedup_union() {
        let t = PhpType::union(vec![PhpType::string(), PhpType::string()]);
        assert_eq!(t.simplified().to_string(), "string");
    }

    #[test]
    fn true_false_becomes_bool() {
        let t = PhpType::union(vec![PhpType::true_(), PhpType::false_()]);
        assert_eq!(t.simplified().to_string(), "bool");
    }

    #[test]
    fn true_false_with_extra_member() {
        let t = PhpType::union(vec![PhpType::true_(), PhpType::false_(), PhpType::null()]);
        let s = t.simplified();
        let display = s.to_string();
        assert!(display.contains("bool"), "should contain bool: {display}");
        assert!(display.contains("null"), "should contain null: {display}");
        assert!(
            !display.contains("true"),
            "should not contain true: {display}"
        );
        assert!(
            !display.contains("false"),
            "should not contain false: {display}"
        );
    }

    #[test]
    fn mixed_absorbs_union() {
        let t = PhpType::union(vec![PhpType::mixed(), PhpType::string(), PhpType::int()]);
        assert_eq!(t.simplified().to_string(), "mixed");
    }

    #[test]
    fn scalar_refinement_absorbed() {
        let t = PhpType::union(vec![PhpType::named(atom("positive-int")), PhpType::int()]);
        assert_eq!(t.simplified().to_string(), "int");
    }

    #[test]
    fn non_empty_string_absorbed_by_string() {
        let t = PhpType::union(vec![
            PhpType::named(atom("non-empty-string")),
            PhpType::string(),
        ]);
        assert_eq!(t.simplified().to_string(), "string");
    }

    #[test]
    fn list_absorbed_by_array() {
        let t = PhpType::union(vec![PhpType::named(atom("list")), PhpType::array()]);
        assert_eq!(t.simplified().to_string(), "array");
    }

    #[test]
    fn single_member_union_unwrapped() {
        let t = PhpType::union(vec![PhpType::int()]);
        assert_eq!(t.simplified(), PhpType::int());
    }

    #[test]
    fn single_member_intersection_unwrapped() {
        let t = PhpType::intersection(vec![PhpType::named(atom("Foo"))]);
        assert_eq!(t.simplified(), PhpType::named(atom("Foo")));
    }

    #[test]
    fn nullable_never_becomes_null() {
        let t = PhpType::nullable(PhpType::never());
        assert_eq!(t.simplified(), PhpType::null());
    }

    #[test]
    fn nullable_null_becomes_null() {
        let t = PhpType::nullable(PhpType::null());
        assert_eq!(t.simplified(), PhpType::null());
    }

    #[test]
    fn nullable_mixed_becomes_mixed() {
        let t = PhpType::nullable(PhpType::mixed());
        assert_eq!(t.simplified(), PhpType::mixed());
    }

    #[test]
    fn nested_union_flattened() {
        let t = PhpType::union(vec![
            PhpType::union(vec![
                PhpType::named(atom("Foo")),
                PhpType::named(atom("Bar")),
            ]),
            PhpType::named(atom("Baz")),
        ]);
        let s = t.simplified();
        if let TypeKind::Union(members) = &s.kind() {
            assert_eq!(members.len(), 3);
        } else {
            panic!("Expected Union, got {s:?}");
        }
    }

    #[test]
    fn nested_intersection_flattened() {
        let t = PhpType::intersection(vec![
            PhpType::intersection(vec![
                PhpType::named(atom("Foo")),
                PhpType::named(atom("Bar")),
            ]),
            PhpType::named(atom("Baz")),
        ]);
        let s = t.simplified();
        if let TypeKind::Intersection(members) = &s.kind() {
            assert_eq!(members.len(), 3);
        } else {
            panic!("Expected Intersection, got {s:?}");
        }
    }

    #[test]
    fn intersection_with_never_collapses() {
        let t = PhpType::intersection(vec![PhpType::named(atom("Foo")), PhpType::never()]);
        assert_eq!(t.simplified(), PhpType::never());
    }

    #[test]
    fn generic_args_simplified() {
        let t = PhpType::generic(
            "array",
            vec![PhpType::union(vec![PhpType::true_(), PhpType::false_()])],
        );
        let s = t.simplified();
        assert_eq!(s.to_string(), "array<bool>");
    }

    #[test]
    fn dedup_case_insensitive() {
        let t = PhpType::union(vec![PhpType::named(atom("String")), PhpType::string()]);
        // Should deduplicate — only one remains.
        let s = t.simplified();
        assert!(
            !matches!(s.kind(), TypeKind::Union(_)),
            "should be unwrapped: {s:?}"
        );
    }

    #[test]
    fn dedup_preserves_case_sensitive_payloads_and_number_class_names() {
        let literal_union = PhpType::union(vec![
            PhpType::literal_string_raw("'A'"),
            PhpType::literal_string_raw("'a'"),
        ]);
        assert_eq!(literal_union.simplified().to_string(), "'A'|'a'");

        let upper = PhpType::generic("Box", vec![PhpType::literal_string_raw("'A'")]);
        let lower = PhpType::generic("box", vec![PhpType::literal_string_raw("'a'")]);
        let generic_union = PhpType::union(vec![upper.clone(), lower.clone()]);
        assert_eq!(
            generic_union.simplified(),
            PhpType::union(vec![upper, lower])
        );

        let number_class = PhpType::union(vec![
            PhpType::named(atom("number")),
            PhpType::named(atom("Number")),
        ]);
        assert_eq!(number_class.simplified(), number_class);

        let static_class_bounds = PhpType::union(vec![
            PhpType::static_type(atom("number")),
            PhpType::static_type(atom("Number")),
        ]);
        assert_eq!(
            static_class_bounds.simplified(),
            PhpType::static_type(atom("number"))
        );

        let generic_class_names = PhpType::union(vec![
            PhpType::generic("number", vec![PhpType::int()]),
            PhpType::generic("Number", vec![PhpType::int()]),
        ]);
        assert_eq!(
            generic_class_names.simplified(),
            PhpType::generic("number", vec![PhpType::int()])
        );
    }

    #[test]
    fn simplification_keeps_one_representative_for_mutual_scalar_aliases() {
        assert_eq!(
            PhpType::union(vec![PhpType::int(), PhpType::named(atom("integer"))]).simplified(),
            PhpType::int()
        );
        assert_eq!(
            PhpType::union(vec![PhpType::float(), PhpType::named(atom("double"))]).simplified(),
            PhpType::float()
        );
    }

    #[test]
    fn dedup_recognizes_equivalent_literal_spellings() {
        assert_eq!(
            PhpType::union(vec![PhpType::literal_int("1"), PhpType::literal_int("0x1"),])
                .simplified(),
            PhpType::literal_int("1")
        );
        assert_eq!(
            PhpType::union(vec![
                PhpType::literal_float("1.0"),
                PhpType::literal_float("1e0"),
            ])
            .simplified(),
            PhpType::literal_float("1.0")
        );
        assert_eq!(
            PhpType::union(vec![
                PhpType::literal_string_raw("'x'"),
                PhpType::literal_string_raw("\"x\""),
            ])
            .simplified(),
            PhpType::literal_string_raw("'x'")
        );
    }

    #[test]
    fn dedup_does_not_equate_escaped_single_and_double_quoted_strings() {
        let escaped = PhpType::union(vec![
            PhpType::literal_string_raw(r"'\n'"),
            PhpType::literal_string_raw(r#""\n""#),
        ])
        .simplified();

        assert_eq!(escaped.union_members().len(), 2);
    }

    #[test]
    fn broad_scalar_absorbs_only_its_matching_literal_kind() {
        assert_eq!(
            PhpType::union(vec![
                PhpType::string(),
                PhpType::literal_string_raw("'foo'")
            ])
            .simplified(),
            PhpType::string()
        );
        assert_eq!(
            PhpType::union(vec![PhpType::int(), PhpType::literal_int("1")]).simplified(),
            PhpType::int()
        );
        assert_eq!(
            PhpType::union(vec![PhpType::float(), PhpType::literal_float("1.5")]).simplified(),
            PhpType::float()
        );
        assert_eq!(
            PhpType::union(vec![PhpType::float(), PhpType::literal_int("1")])
                .simplified()
                .to_string(),
            "float|1"
        );
    }

    #[test]
    fn runtime_value_join_separates_value_sets_from_parameter_coercion() {
        assert_eq!(
            PhpType::join_runtime_value_types(vec![PhpType::int(), PhpType::float()]).to_string(),
            "int|float"
        );
        assert_eq!(
            PhpType::join_runtime_value_types(vec![PhpType::float(), PhpType::literal_int("1"),])
                .to_string(),
            "float|1"
        );
        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::literal_int("1"),
                PhpType::int(),
                PhpType::float(),
            ])
            .to_string(),
            "int|float"
        );
        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::int(),
                PhpType::named(atom("integer")),
            ]),
            PhpType::int()
        );
    }

    #[test]
    fn runtime_value_join_absorbs_real_value_subsets() {
        assert_eq!(
            PhpType::join_runtime_value_types(vec![PhpType::numeric(), PhpType::literal_int("1"),]),
            PhpType::numeric()
        );
        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::named(atom("scalar")),
                PhpType::literal_string_raw("'x'"),
            ]),
            PhpType::named(atom("scalar"))
        );
        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::named(atom("array-key")),
                PhpType::literal_int("1"),
            ]),
            PhpType::named(atom("array-key"))
        );
        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::parse("int<1, 10>"),
                PhpType::literal_int("5"),
            ]),
            PhpType::parse("int<1, 10>")
        );

        let number_and_class = PhpType::join_runtime_value_types(vec![
            PhpType::named(atom("number")),
            PhpType::named(atom("Number")),
        ]);
        assert_eq!(number_and_class.to_string(), "number|Number");

        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::named(atom("real")),
                PhpType::literal_float("1.5"),
            ]),
            PhpType::named(atom("real"))
        );
        assert_eq!(
            PhpType::union(vec![
                PhpType::named(atom("real")),
                PhpType::literal_float("1.5"),
            ])
            .simplified(),
            PhpType::named(atom("real"))
        );
        assert!(PhpType::literal_float("1.5").is_subtype_of(&PhpType::named(atom("real"))));
    }

    /// `real` is a float alias only in lowercase. A userland class named `Real`
    /// is a class like any other, so it must not be folded into `float`.
    #[test]
    fn capitalised_real_is_a_class_not_the_float_alias() {
        let class = PhpType::named(atom("Real"));

        assert!(!class.is_float());
        assert!(!class.is_subtype_of(&PhpType::float()));
        assert!(!PhpType::float().is_subtype_of(&class));
        assert!(!PhpType::literal_float("1.5").is_subtype_of(&class));
        assert!(!PhpType::named(atom("real")).is_subtype_of(&class));
        assert!(!class.is_subtype_of(&PhpType::named(atom("real"))));

        // Class names themselves stay case-insensitive.
        assert!(class.is_subtype_of(&PhpType::named(atom("REAL"))));

        let with_float = PhpType::union(vec![class.clone(), PhpType::literal_float("1.5")]);
        assert_eq!(with_float.simplified(), with_float);
    }

    #[test]
    fn review_regression_runtime_join_preserves_structured_coercion_domains() {
        let joined = PhpType::join_runtime_value_types(vec![
            PhpType::literal_string_raw("'tag'"),
            PhpType::parse("array<int>"),
            PhpType::parse("array<float>"),
            PhpType::parse("callable(): int"),
            PhpType::parse("callable(): float"),
        ]);

        let members = joined.union_members();
        assert_eq!(members.len(), 5);
        for expected in [
            PhpType::literal_string_raw("'tag'"),
            PhpType::parse("array<int>"),
            PhpType::parse("array<float>"),
            PhpType::parse("callable(): int"),
            PhpType::parse("callable(): float"),
        ] {
            assert!(
                members.contains(&&expected),
                "runtime join dropped structured alternative {expected}: {joined}"
            );
        }
    }

    #[test]
    fn review_regression_runtime_join_absorbs_only_numeric_string_literals_into_numeric() {
        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::numeric(),
                PhpType::literal_string_raw("'1'"),
            ]),
            PhpType::numeric()
        );
        assert_eq!(
            PhpType::join_runtime_value_types(vec![
                PhpType::numeric(),
                PhpType::literal_string_raw("'x'"),
            ])
            .to_string(),
            "numeric|'x'"
        );
    }

    #[test]
    fn review_regression_semantic_dedup_keeps_unparseable_numeric_literals_distinct() {
        let overflow = PhpType::union(vec![
            PhpType::literal_int("9223372036854775808"),
            PhpType::literal_int("9223372036854775809"),
        ])
        .simplified();
        assert_eq!(overflow.union_members().len(), 2);

        let malformed = PhpType::union(vec![
            PhpType::literal_float("not-a-float"),
            PhpType::literal_float("still-not-a-float"),
        ])
        .simplified();
        assert_eq!(malformed.union_members().len(), 2);
    }

    #[test]
    fn closure_subtype_of_callable() {
        // Ensure Closure <: callable works in subtype check (case-insensitive).
        assert!(PhpType::named(atom("closure")).is_subtype_of(&PhpType::callable()));
    }
}

// ── Intersection distribution tests ─────────────────────────────────

mod distribute_tests {
    use super::*;

    #[test]
    fn distribute_simple() {
        // (A|B) & C → (A&C) | (B&C)
        let t = PhpType::intersection(vec![
            PhpType::union(vec![PhpType::named(atom("A")), PhpType::named(atom("B"))]),
            PhpType::named(atom("C")),
        ]);
        let d = t.distribute_intersection();
        if let TypeKind::Union(members) = &d.kind() {
            assert_eq!(members.len(), 2);
        } else {
            panic!("Expected Union, got {d:?}");
        }
    }

    #[test]
    fn distribute_no_union_unchanged() {
        let t = PhpType::intersection(vec![
            PhpType::named(atom("Foo")),
            PhpType::named(atom("Bar")),
        ]);
        let d = t.distribute_intersection();
        assert_eq!(d, t);
    }

    #[test]
    fn distribute_non_intersection_unchanged() {
        let t = PhpType::named(atom("Foo"));
        let d = t.distribute_intersection();
        assert_eq!(d, t);
    }

    #[test]
    fn distribute_two_unions() {
        // (A|B) & (C|D) → (A&C) | (A&D) | (B&C) | (B&D)
        let t = PhpType::intersection(vec![
            PhpType::union(vec![PhpType::named(atom("A")), PhpType::named(atom("B"))]),
            PhpType::union(vec![PhpType::named(atom("C")), PhpType::named(atom("D"))]),
        ]);
        let d = t.distribute_intersection();
        if let TypeKind::Union(members) = &d.kind() {
            assert_eq!(members.len(), 4, "Expected 4 members, got {d}");
        } else {
            panic!("Expected Union, got {d:?}");
        }
    }

    #[test]
    fn distribute_with_simplification() {
        // (A|A) & B → after distribution and simplification → A & B
        let t = PhpType::intersection(vec![
            PhpType::union(vec![PhpType::named(atom("A")), PhpType::named(atom("A"))]),
            PhpType::named(atom("B")),
        ]);
        let d = t.distribute_intersection();
        // The union (A|A) deduplicates to A, so the result should be A&B.
        assert!(
            matches!(d.kind(), TypeKind::Intersection(_)),
            "Expected Intersection, got {d:?}"
        );
    }
}

// ── Predicate tests ─────────────────────────────────────────────────

mod predicate_tests {
    use super::*;

    // ── is_bool ─────────────────────────────────────────────────

    #[test]
    fn is_bool_true_for_bool() {
        assert!(PhpType::bool().is_bool());
    }

    #[test]
    fn is_bool_true_for_boolean() {
        assert!(PhpType::named(atom("boolean")).is_bool());
    }

    #[test]
    fn is_bool_case_insensitive() {
        // `bool` is one of PHP's actual reserved type keywords, so no class
        // can ever be named `Bool` in any casing.
        assert!(PhpType::named(atom("Bool")).is_bool());
        assert!(PhpType::named(atom("BOOL")).is_bool());
    }

    #[test]
    fn is_bool_not_folded_for_uppercase_boolean() {
        // `boolean` is only a legacy alias, not a reserved keyword, so PHP
        // allows a class named `Boolean`; only the exact lowercase spelling
        // is the alias (see `is_lowercase_only_pseudo_type`).
        assert!(!PhpType::named(atom("Boolean")).is_bool());
        assert!(!PhpType::named(atom("BOOLEAN")).is_bool());
    }

    #[test]
    fn is_bool_nullable() {
        assert!(PhpType::nullable(PhpType::bool()).is_bool());
    }

    #[test]
    fn is_bool_false_for_int() {
        assert!(!PhpType::int().is_bool());
    }

    #[test]
    fn is_bool_false_for_true() {
        assert!(!PhpType::true_().is_bool());
    }

    // ── is_true ────────────────────────────────────────────────

    #[test]
    fn is_true_true_for_true() {
        assert!(PhpType::true_().is_true());
    }

    #[test]
    fn is_true_case_insensitive() {
        assert!(PhpType::named(atom("True")).is_true());
        assert!(PhpType::named(atom("TRUE")).is_true());
    }

    #[test]
    fn is_true_nullable() {
        assert!(PhpType::nullable(PhpType::true_()).is_true());
    }

    #[test]
    fn is_true_false_for_false() {
        assert!(!PhpType::false_().is_true());
    }

    #[test]
    fn is_true_false_for_bool() {
        assert!(!PhpType::bool().is_true());
    }

    // ── is_false ───────────────────────────────────────────────

    #[test]
    fn is_false_true_for_false() {
        assert!(PhpType::false_().is_false());
    }

    #[test]
    fn is_false_case_insensitive() {
        assert!(PhpType::named(atom("False")).is_false());
        assert!(PhpType::named(atom("FALSE")).is_false());
    }

    #[test]
    fn is_false_nullable() {
        assert!(PhpType::nullable(PhpType::false_()).is_false());
    }

    #[test]
    fn is_false_false_for_true() {
        assert!(!PhpType::true_().is_false());
    }

    #[test]
    fn is_false_false_for_bool() {
        assert!(!PhpType::bool().is_false());
    }

    // ── is_int ─────────────────────────────────────────────────

    #[test]
    fn is_int_true_for_int() {
        assert!(PhpType::int().is_int());
    }

    #[test]
    fn is_int_true_for_integer() {
        assert!(PhpType::named(atom("integer")).is_int());
    }

    #[test]
    fn is_int_case_insensitive() {
        // `int` is one of PHP's actual reserved type keywords, so no class
        // can ever be named `Int` in any casing.
        assert!(PhpType::named(atom("Int")).is_int());
        assert!(PhpType::named(atom("INT")).is_int());
    }

    #[test]
    fn is_int_not_folded_for_uppercase_integer() {
        // `integer` is only a legacy alias, not a reserved keyword, so PHP
        // allows a class named `Integer`; only the exact lowercase spelling
        // is the alias (see `is_lowercase_only_pseudo_type`).
        assert!(!PhpType::named(atom("Integer")).is_int());
        assert!(!PhpType::named(atom("INTEGER")).is_int());
    }

    #[test]
    fn is_int_nullable() {
        assert!(PhpType::nullable(PhpType::int()).is_int());
    }

    #[test]
    fn is_int_false_for_float() {
        assert!(!PhpType::float().is_int());
    }

    // ── is_string_type ─────────────────────────────────────────

    #[test]
    fn is_string_type_true_for_string() {
        assert!(PhpType::string().is_string_type());
    }

    #[test]
    fn is_string_type_case_insensitive() {
        assert!(PhpType::named(atom("String")).is_string_type());
        assert!(PhpType::named(atom("STRING")).is_string_type());
    }

    #[test]
    fn is_string_type_nullable() {
        assert!(PhpType::nullable(PhpType::string()).is_string_type());
    }

    #[test]
    fn is_string_type_false_for_int() {
        assert!(!PhpType::int().is_string_type());
    }

    #[test]
    fn is_string_type_false_for_class_string() {
        assert!(!PhpType::class_string(None).is_string_type());
    }

    // ── is_float ───────────────────────────────────────────────

    #[test]
    fn is_float_true_for_float() {
        assert!(PhpType::float().is_float());
    }

    #[test]
    fn is_float_true_for_double() {
        assert!(PhpType::named(atom("double")).is_float());
    }

    #[test]
    fn is_float_case_insensitive() {
        // `float` is one of PHP's actual reserved type keywords, so no class
        // can ever be named `Float` in any casing.
        assert!(PhpType::named(atom("Float")).is_float());
        assert!(PhpType::named(atom("FLOAT")).is_float());
    }

    #[test]
    fn is_float_not_folded_for_uppercase_double() {
        // `double` is only a legacy alias, not a reserved keyword, so PHP
        // allows a class named `Double`; only the exact lowercase spelling
        // is the alias (see `is_lowercase_only_pseudo_type`).
        assert!(!PhpType::named(atom("Double")).is_float());
        assert!(!PhpType::named(atom("DOUBLE")).is_float());
    }

    #[test]
    fn is_float_nullable() {
        assert!(PhpType::nullable(PhpType::float()).is_float());
    }

    #[test]
    fn is_float_false_for_int() {
        assert!(!PhpType::int().is_float());
    }

    // ── is_object ──────────────────────────────────────────────

    #[test]
    fn is_object_true_for_object() {
        assert!(PhpType::object().is_object());
    }

    #[test]
    fn is_object_case_insensitive() {
        assert!(PhpType::named(atom("Object")).is_object());
        assert!(PhpType::named(atom("OBJECT")).is_object());
    }

    #[test]
    fn is_object_nullable() {
        assert!(PhpType::nullable(PhpType::object()).is_object());
    }

    #[test]
    fn is_object_false_for_class() {
        assert!(!PhpType::named(atom("User")).is_object());
    }

    #[test]
    fn is_object_false_for_object_shape() {
        assert!(!PhpType::object_shape(vec![]).is_object());
    }

    // ── is_callable ────────────────────────────────────────────

    #[test]
    fn is_callable_true_for_callable() {
        assert!(PhpType::callable().is_callable());
    }

    #[test]
    fn is_callable_case_insensitive() {
        assert!(PhpType::named(atom("Callable")).is_callable());
        assert!(PhpType::named(atom("CALLABLE")).is_callable());
    }

    #[test]
    fn is_callable_true_for_closure() {
        assert!(PhpType::named(atom("Closure")).is_callable());
        assert!(PhpType::named(atom("closure")).is_callable());
    }

    #[test]
    fn is_callable_true_for_callable_variant() {
        let t = PhpType::callable_spec("callable", vec![], None);
        assert!(t.is_callable());
    }

    #[test]
    fn is_callable_true_for_closure_variant() {
        let t = PhpType::callable_spec("Closure", vec![], Some(PhpType::void()));
        assert!(t.is_callable());
    }

    #[test]
    fn is_callable_nullable() {
        assert!(PhpType::nullable(PhpType::callable()).is_callable());
        assert!(PhpType::nullable(PhpType::named(atom("Closure"))).is_callable());
    }

    #[test]
    fn is_callable_false_for_string() {
        assert!(!PhpType::string().is_callable());
    }

    // ── is_self_like ───────────────────────────────────────────

    #[test]
    fn is_self_like_true_for_self() {
        assert!(PhpType::self_().is_self_like());
    }

    #[test]
    fn is_self_like_true_for_static() {
        assert!(PhpType::static_().is_self_like());
    }

    #[test]
    fn is_self_like_true_for_this() {
        assert!(PhpType::named(atom("$this")).is_self_like());
    }

    #[test]
    fn is_self_like_true_for_parent() {
        assert!(PhpType::parent_().is_self_like());
    }

    #[test]
    fn is_self_like_case_insensitive() {
        assert!(PhpType::named(atom("Self")).is_self_like());
        assert!(PhpType::named(atom("STATIC")).is_self_like());
        assert!(PhpType::named(atom("Parent")).is_self_like());
    }

    #[test]
    fn is_self_like_nullable() {
        assert!(PhpType::nullable(PhpType::static_()).is_self_like());
    }

    #[test]
    fn is_self_like_false_for_class() {
        assert!(!PhpType::named(atom("User")).is_self_like());
    }

    #[test]
    fn is_self_like_false_for_int() {
        assert!(!PhpType::int().is_self_like());
    }

    #[test]
    fn is_self_ref_true_for_self() {
        assert!(PhpType::self_().is_self_ref());
    }

    #[test]
    fn is_self_ref_true_for_static() {
        assert!(PhpType::static_().is_self_ref());
    }

    #[test]
    fn is_self_ref_true_for_this() {
        assert!(PhpType::named(atom("$this")).is_self_ref());
    }

    #[test]
    fn is_self_ref_false_for_parent() {
        assert!(!PhpType::parent_().is_self_ref());
    }

    #[test]
    fn is_self_ref_case_insensitive() {
        assert!(PhpType::named(atom("SELF")).is_self_ref());
        assert!(PhpType::named(atom("Static")).is_self_ref());
    }

    #[test]
    fn is_self_ref_not_nullable() {
        assert!(!PhpType::nullable(PhpType::static_()).is_self_ref());
    }

    #[test]
    fn is_self_ref_false_for_class() {
        assert!(!PhpType::named(atom("Foo")).is_self_ref());
    }

    // ── is_bool/is_true/is_false for non-matching types ────────

    #[test]
    fn predicates_false_for_union() {
        let u = PhpType::union(vec![PhpType::int(), PhpType::string()]);
        assert!(!u.is_bool());
        assert!(!u.is_true());
        assert!(!u.is_false());
        assert!(!u.is_int());
        assert!(!u.is_string_type());
        assert!(!u.is_float());
        assert!(!u.is_object());
        assert!(!u.is_array_key());
        assert!(!u.is_callable());
        assert!(!u.is_self_like());
    }

    // ── is_array_key ───────────────────────────────────────────

    #[test]
    fn is_array_key_true_for_array_key() {
        assert!(PhpType::parse("array-key").is_array_key());
    }

    #[test]
    fn is_array_key_case_insensitive() {
        assert!(PhpType::parse("Array-Key").is_array_key());
        assert!(PhpType::parse("ARRAY-KEY").is_array_key());
    }

    #[test]
    fn is_array_key_nullable() {
        assert!(PhpType::parse("?array-key").is_array_key());
    }

    #[test]
    fn is_array_key_false_for_int() {
        assert!(!PhpType::parse("int").is_array_key());
    }

    #[test]
    fn is_array_key_false_for_string() {
        assert!(!PhpType::parse("string").is_array_key());
    }

    // ── is_iterable ────────────────────────────────────────────

    #[test]
    fn is_iterable_true_for_iterable() {
        assert!(PhpType::parse("iterable").is_iterable());
    }

    #[test]
    fn is_iterable_case_insensitive() {
        assert!(PhpType::parse("Iterable").is_iterable());
        assert!(PhpType::parse("ITERABLE").is_iterable());
    }

    #[test]
    fn is_iterable_nullable() {
        assert!(PhpType::parse("?iterable").is_iterable());
    }

    #[test]
    fn is_iterable_false_for_array() {
        assert!(!PhpType::parse("array").is_iterable());
    }

    #[test]
    fn is_iterable_false_for_iterator() {
        assert!(!PhpType::parse("Iterator").is_iterable());
    }

    // ── is_closure ─────────────────────────────────────────────

    #[test]
    fn is_closure_true_for_closure() {
        assert!(PhpType::parse("Closure").is_closure());
    }

    #[test]
    fn is_closure_true_for_fqn_closure() {
        assert!(PhpType::parse("\\Closure").is_closure());
    }

    #[test]
    fn is_closure_case_insensitive() {
        assert!(PhpType::parse("closure").is_closure());
        assert!(PhpType::parse("CLOSURE").is_closure());
    }

    #[test]
    fn is_closure_nullable() {
        assert!(PhpType::parse("?Closure").is_closure());
    }

    #[test]
    fn is_closure_callable_variant() {
        let ty = PhpType::callable_spec("Closure", vec![], Some(PhpType::void()));
        assert!(ty.is_closure());
    }

    #[test]
    fn is_closure_false_for_callable() {
        assert!(!PhpType::parse("callable").is_closure());
    }

    #[test]
    fn is_closure_false_for_string() {
        assert!(!PhpType::parse("string").is_closure());
    }

    // ── is_resource ────────────────────────────────────────────

    #[test]
    fn is_resource_true_for_resource() {
        assert!(PhpType::parse("resource").is_resource());
    }

    #[test]
    fn is_resource_not_folded_for_uppercase_resource() {
        // PHP has no `resource` type-hint at all, so `resource` is not a
        // reserved keyword in any casing; a project may legally declare a
        // class named `Resource`, so only the exact lowercase spelling is
        // the pseudo-type (see `is_lowercase_only_pseudo_type`).
        assert!(!PhpType::parse("Resource").is_resource());
        assert!(!PhpType::parse("RESOURCE").is_resource());
    }

    #[test]
    fn is_resource_nullable() {
        assert!(PhpType::parse("?resource").is_resource());
    }

    #[test]
    fn is_resource_false_for_string() {
        assert!(!PhpType::parse("string").is_resource());
    }

    // ── is_empty_sentinel ──────────────────────────────────────

    #[test]
    fn is_empty_sentinel_true() {
        assert!(PhpType::named(atom("__empty")).is_empty_sentinel());
        assert!(PhpType::empty_sentinel().is_empty_sentinel());
    }

    #[test]
    fn is_named_case_sensitive() {
        assert!(PhpType::named(atom("TModel")).is_named("TModel"));
        assert!(!PhpType::named(atom("TModel")).is_named("tmodel"));
        assert!(PhpType::int().is_named("int"));
        assert!(!PhpType::int().is_named("INT"));
    }

    #[test]
    fn is_named_false_for_non_named() {
        assert!(!PhpType::generic("list", vec![PhpType::int()]).is_named("list"));
        assert!(!PhpType::nullable(PhpType::named(atom("Foo"))).is_named("Foo"));
    }

    #[test]
    fn is_named_ci_case_insensitive() {
        assert!(PhpType::named(atom("stdClass")).is_named_ci("stdclass"));
        assert!(PhpType::named(atom("stdclass")).is_named_ci("stdClass"));
        assert!(!PhpType::named(atom("Foo")).is_named_ci("Bar"));
    }

    #[test]
    fn list_constructor() {
        let ty = PhpType::list(PhpType::string());
        assert_eq!(ty.to_string(), "list<string>");
    }

    #[test]
    fn generic_array_constructor() {
        let ty = PhpType::generic_array(PhpType::string(), PhpType::int());
        assert_eq!(ty.to_string(), "array<string, int>");
    }

    #[test]
    fn generic_array_val_constructor() {
        let ty = PhpType::generic_array_val(PhpType::int());
        assert_eq!(ty.to_string(), "array<int>");
    }

    #[test]
    fn is_empty_sentinel_false_for_regular() {
        assert!(!PhpType::parse("string").is_empty_sentinel());
        assert!(!PhpType::parse("").is_empty_sentinel());
    }

    #[test]
    fn is_string_literal_single_quoted() {
        assert!(PhpType::literal_string_raw("'hello'").is_string_literal());
    }

    #[test]
    fn is_string_literal_double_quoted() {
        assert!(PhpType::literal_string_raw("\"world\"").is_string_literal());
    }

    #[test]
    fn is_string_literal_false_for_int() {
        assert!(!PhpType::literal_int("42").is_string_literal());
    }

    #[test]
    fn is_string_literal_false_for_named() {
        assert!(!PhpType::named(atom("string")).is_string_literal());
    }

    #[test]
    fn is_int_literal_positive() {
        assert!(PhpType::literal_int("42").is_int_literal());
    }

    #[test]
    fn is_int_literal_negative() {
        assert!(PhpType::literal_int("-1").is_int_literal());
    }

    #[test]
    fn is_int_literal_false_for_string() {
        assert!(!PhpType::literal_string_raw("'hello'").is_int_literal());
    }

    #[test]
    fn is_int_literal_false_for_named() {
        assert!(!PhpType::named(atom("int")).is_int_literal());
    }
}

// ─── Parenthesized type parsing ─────────────────────────────────────

#[test]
fn parse_grouped_union_array() {
    // `(string|int)[]` should parse as Array(Union(string, int))
    let ty = PhpType::parse("(string|int)[]");
    assert!(
        matches!(ty.kind(), TypeKind::Array(inner) if matches!(inner.kind(), TypeKind::Union(_))),
        "expected Array(Union(...)), got: {:?}",
        ty
    );
}

#[test]
fn parse_parenthesized_callable() {
    // `(callable(): string)` should parse and unwrap the parens
    let ty = PhpType::parse("(callable(): string)");
    assert!(
        matches!(&ty.kind(), TypeKind::Callable { .. }),
        "expected Callable, got: {:?}",
        ty
    );
}

#[test]
fn parse_callable_return_string() {
    let ty = PhpType::parse("callable():string");
    assert!(
        matches!(&ty.kind(), TypeKind::Callable { .. }),
        "expected Callable, got: {:?}",
        ty
    );
}

/// `PhpType` is embedded millions of times over a large project (once per
/// parameter, return, and property type, plus every nested position inside
/// them), so its size is load-bearing for peak memory. It must stay a bare
/// handle: anything stored beside the pointer widens every slot in the
/// program.
#[test]
fn php_type_size_is_bounded() {
    assert_eq!(
        size_of::<PhpType>(),
        8,
        "PhpType grew past a single pointer — store the new data on \
         `TypeKind` instead, where it costs one copy per distinct type"
    );
    // `Arc`'s non-null niche keeps the optional form the same size, which is
    // what `MethodInfo`/`ParameterInfo` actually store.
    assert_eq!(size_of::<Option<PhpType>>(), size_of::<PhpType>());
    // One node per distinct type, so this is far less load-bearing than the
    // handle, but it still bounds the interner's own footprint.
    assert_eq!(
        size_of::<TypeKind>(),
        24,
        "TypeKind grew past two pointers plus a discriminant — box the new \
         variant's payload instead (see the type's Size doc comment)"
    );
}

/// Structural equality has to imply pointer equality, because `PartialEq`
/// is a pointer comparison. Every construction path must therefore go
/// through the interner.
#[test]
fn structurally_equal_types_share_one_allocation() {
    let parsed = PhpType::parse("array<int, Foo>|null");
    let built = PhpType::union(vec![
        PhpType::generic("array", vec![PhpType::int(), PhpType::named(atom("Foo"))]),
        PhpType::null(),
    ]);
    assert_eq!(parsed, built);
    assert!(std::ptr::eq(parsed.kind(), built.kind()));

    // Distinct forms must stay distinct.
    assert_ne!(parsed, PhpType::parse("array<int, Foo>"));
}

/// The table holds weak references, so a form nothing refers to any more is
/// reclaimed rather than leaked. `Raw` holds free-text parse fallbacks and
/// `Literal` holds source literals, so a server that re-analyses a file on
/// every keystroke would otherwise grow without bound.
///
/// Churns far more distinct forms than the table is allowed to end up
/// holding, rather than reading an exact count: other tests intern
/// concurrently in the same process.
#[test]
fn dropped_types_leave_the_interner() {
    const CHURN: usize = 20_000;

    let before = crate::php_type::interned_len();
    for i in 0..CHURN {
        // Each form is dropped before the next is built, so at most one of
        // them is ever live.
        let churned = PhpType::raw(format!("churn-{i}"));
        assert_eq!(churned.to_string(), format!("churn-{i}"));
    }
    let after = crate::php_type::interned_len();

    assert!(
        after < before + CHURN / 10,
        "interner grew from {before} to {after} across {CHURN} dropped \
         forms — dead entries are not being reclaimed"
    );
}

// ---------------------------------------------------------------------------
// Unmodelled PHPDoc pseudo-types
// ---------------------------------------------------------------------------

#[test]
fn unrecognized_hyphenated_spelling_becomes_mixed() {
    // Hyphens are not valid PHP identifier characters, so none of these can
    // name a class.  Keeping the spelling would send it through class
    // resolution, where it is qualified against the file's namespace and then
    // enforced as a type no value can satisfy.
    for spelling in [
        "decimal-int-string",
        "non-decimal-int-string",
        "static-closure",
        "static-pure-closure",
        "non-null-mixed",
        "arraylike-object",
    ] {
        assert_eq!(
            PhpType::parse(spelling),
            PhpType::mixed(),
            "{spelling} should degrade to mixed"
        );
    }
}

#[test]
fn unrecognized_hyphenated_generic_becomes_mixed() {
    for spelling in [
        "non-empty-associative-array<string, int>",
        "int-range<0, 255>",
    ] {
        assert_eq!(
            PhpType::parse(spelling),
            PhpType::mixed(),
            "{spelling} should degrade to mixed"
        );
    }
}

#[test]
fn modelled_hyphenated_keywords_keep_their_spelling() {
    // The degradation must not swallow the pseudo-types we do model.
    for spelling in [
        "non-empty-string",
        "positive-int",
        "class-string",
        "array-key",
        "non-empty-list",
        "int-mask-of<Foo::*>",
    ] {
        assert_ne!(
            PhpType::parse(spelling),
            PhpType::mixed(),
            "{spelling} is a keyword we model and must survive parsing"
        );
    }
}

#[test]
fn unmodelled_refinements_widen_to_the_type_they_refine() {
    // These spellings are recognized; the property each adds (literalness,
    // purity, having __toString) is not one we can decide, so each widens to
    // the type it refines rather than being enforced or thrown away.
    for (spelling, expected) in [
        ("literal-int", "int"),
        ("literal-float", "float"),
        ("stringable-object", "object"),
        ("pure-callable", "callable"),
        ("lowercase-callable-string", "callable-string"),
        ("uppercase-callable-string", "callable-string"),
    ] {
        assert_eq!(PhpType::parse(spelling).to_string(), expected, "{spelling}");
    }
    // `Closure` is a class, so the widened form has to be fully qualified or
    // it would be resolved against the docblock's own namespace.
    assert_eq!(PhpType::parse("pure-closure").to_string(), "\\Closure");
}

#[test]
fn pure_callable_specification_keeps_its_signature() {
    // Only the purity is unmodelled — the parameter and return types are not.
    assert_eq!(
        PhpType::parse("pure-callable(int): string").to_string(),
        "callable(int): string"
    );
}

// ---------------------------------------------------------------------------
// key-of / value-of evaluation
// ---------------------------------------------------------------------------

#[test]
fn key_of_concrete_shape_is_evaluated_at_parse_time() {
    // Nothing else runs before a declared type is enforced, so an operand that
    // is already concrete has to be projected here or every call site is
    // checked against a type expression instead of a set of values.
    assert_eq!(
        PhpType::parse("key-of<array{name: string, age: int}>").to_string(),
        "'name'|'age'"
    );
    assert_eq!(
        PhpType::parse("value-of<array{a: int, b: int}>").to_string(),
        "int"
    );
    assert_eq!(
        PhpType::parse("value-of<array{a: int, b: string}>").to_string(),
        "int|string"
    );
    assert_eq!(PhpType::parse("key-of<list<string>>").to_string(), "int");
}

#[test]
fn key_of_unevaluated_operand_survives_for_a_later_pass() {
    // A template that has not been substituted yet must stay a `key-of` so
    // `substitute` can finish the projection at the call site.
    assert!(matches!(
        PhpType::parse("key-of<T>").kind(),
        TypeKind::KeyOf(_)
    ));
    assert!(matches!(
        PhpType::parse("value-of<Config::MAP>").kind(),
        TypeKind::ValueOf(_)
    ));
}

#[test]
fn unevaluated_operators_widen_to_their_bounds() {
    // An operator whose operand we could not read is an unknown type, not an
    // empty one: `key-of` lands somewhere in `array-key`, `value-of` and an
    // index access anywhere at all.
    assert_eq!(
        PhpType::parse("key-of<T>")
            .unevaluated_operators_as_bounds()
            .to_string(),
        "array-key"
    );
    assert_eq!(
        PhpType::parse("value-of<T>")
            .unevaluated_operators_as_bounds()
            .to_string(),
        "mixed"
    );
    // Nested inside a wrapper, and left alone when there is nothing to widen.
    assert_eq!(
        PhpType::parse("list<key-of<T>>")
            .unevaluated_operators_as_bounds()
            .to_string(),
        "list<array-key>"
    );
    let concrete = PhpType::parse("list<string>");
    assert_eq!(concrete.unevaluated_operators_as_bounds(), concrete);
}

#[test]
fn mixed_is_found_wherever_it_reaches_the_top_level() {
    // `mixed` is the top type, so a union or nullable holding it holds
    // every value and reads as plain `mixed`.
    assert!(PhpType::mixed().contains_mixed());
    assert!(PhpType::parse("mixed|Foo").contains_mixed());
    assert!(PhpType::parse("?mixed").contains_mixed());
    assert!(PhpType::parse("int|mixed|string").contains_mixed());
    assert_eq!(
        PhpType::parse("mixed|Foo").contains_mixed(),
        PhpType::parse("mixed|Foo").simplified().is_mixed(),
        "the cheap answer must match the folded one"
    );
    // A `mixed` nested inside a type argument describes the members of a
    // container, not the container, so it does not reach the top level.
    assert!(!PhpType::parse("array<string, mixed>").contains_mixed());
    assert!(!PhpType::parse("Foo|null").contains_mixed());
    assert!(!PhpType::parse("int").contains_mixed());
}

#[test]
fn benevolence_is_invisible_to_readers_and_matchers() {
    let ty = PhpType::parse("__benevolent<string|false>");
    assert!(ty.is_benevolent());
    // Nothing downstream should be able to tell the difference: the marker
    // is a note about provenance, not a shape.
    assert_eq!(ty.to_string(), "string|false");
    assert!(matches!(ty.kind(), TypeKind::Union(members) if members.len() == 2));
    assert_eq!(ty.strip_benevolence(), PhpType::parse("string|false"));
}

#[test]
fn benevolence_needs_a_branch_to_be_lenient_about() {
    // A lone type has no failure branch to waive, so it is left untagged
    // rather than carrying a marker that can never mean anything.
    assert!(!PhpType::parse("__benevolent<Foo>").is_benevolent());
    assert!(!PhpType::benevolent(PhpType::string()).is_benevolent());
    assert!(PhpType::benevolent(PhpType::parse("string|false")).is_benevolent());
}

#[test]
fn benevolence_survives_a_rewrite() {
    let ty = PhpType::parse("__benevolent<Foo|false>");
    let resolved = ty.resolve_names(&|name| format!("App\\{name}"));
    assert!(
        resolved.is_benevolent(),
        "resolving names must not drop the marker, got {resolved:?}"
    );
    assert_eq!(resolved.to_string(), "App\\Foo|false");
    assert!(ty.shorten().is_benevolent());
}

#[test]
fn a_union_inside_an_intersection_keeps_its_parentheses() {
    let ty = PhpType::parse("(FunctionNode|MethodNode)&MockObject");
    assert_eq!(ty.to_string(), "(FunctionNode|MethodNode)&MockObject");
    // Without the parentheses `&` would bind tighter than `|` on the way
    // back in, dropping the intersection from the first branch.
    assert_eq!(PhpType::parse(&ty.to_string()), ty);
}

#[test]
fn a_union_drops_repeated_alternatives() {
    let user = PhpType::named(atom("App\\Entity\\User"));
    assert_eq!(
        PhpType::union(vec![PhpType::string(), user.clone(), PhpType::string()]).to_string(),
        "string|App\\Entity\\User"
    );
    assert_eq!(
        PhpType::union(vec![
            PhpType::null(),
            PhpType::string(),
            PhpType::array(),
            PhpType::null(),
        ])
        .to_string(),
        "null|string|array"
    );
    // Type names are case-insensitive, so a differently-spelled repeat is
    // still a repeat — but a literal payload's case is part of its value.
    assert_eq!(
        PhpType::union(vec![PhpType::named(atom("String")), PhpType::string()]).to_string(),
        "String"
    );
    assert_eq!(
        PhpType::union(vec![
            PhpType::literal_string_raw("'A'"),
            PhpType::literal_string_raw("'a'"),
        ])
        .to_string(),
        "'A'|'a'"
    );
    // A union left with one alternative is that alternative.
    assert_eq!(
        PhpType::union(vec![PhpType::string(), PhpType::string()]),
        PhpType::string()
    );
}

#[test]
fn a_native_declaration_rules_out_the_alternatives_it_forbids() {
    let inherited = PhpType::parse("array<string, mixed>|list<mixed>|string");
    assert_eq!(
        inherited
            .without_alternatives_the_native_type_forbids(&PhpType::array())
            .to_string(),
        "array<string, mixed>|list<mixed>"
    );
    // A nullable native type keeps the null half.
    assert_eq!(
        inherited
            .without_alternatives_the_native_type_forbids(&PhpType::parse("?string"))
            .to_string(),
        "string"
    );
    // A class name spans no single value domain (it may be `Traversable`,
    // `Stringable`, or invokable), so it survives whatever the native type
    // says while its unmistakably-scalar sibling does not.
    assert_eq!(
        PhpType::parse("Traversable|string")
            .without_alternatives_the_native_type_forbids(&PhpType::array())
            .to_string(),
        "Traversable"
    );
    // `object`, `callable`, and `iterable` are equally cross-cutting on the
    // native side, so a declaration naming one rules out nothing.
    assert_eq!(
        inherited
            .without_alternatives_the_native_type_forbids(&PhpType::parse("iterable"))
            .to_string(),
        inherited.to_string()
    );
    // A native `float` still accepts the `int` a body may produce.
    assert_eq!(
        PhpType::parse("int|float|string")
            .without_alternatives_the_native_type_forbids(&PhpType::float())
            .to_string(),
        "int|float"
    );
}

#[test]
fn truthy_type_strips_null_from_every_nullable_union_member() {
    // A lone nullable member loses its `null` the same way a bare `?T`
    // passed directly to `truthy_type` does.
    assert_eq!(
        PhpType::parse("?int|?bool|?string|?DateTime")
            .truthy_type()
            .unwrap()
            .to_string(),
        "int|true|string|DateTime"
    );
    // A member that is certainly falsy on its own (`null`, `false`) is
    // dropped outright rather than surviving as an empty type.
    assert_eq!(
        PhpType::parse("null|false|?int")
            .truthy_type()
            .unwrap()
            .to_string(),
        "int"
    );
    // A union that is entirely falsy has no truthy half at all.
    assert_eq!(PhpType::parse("null|false").truthy_type(), None);
}

#[test]
fn falsy_type_keeps_only_what_the_skipped_branch_could_hold() {
    // The mirror of the truthy side: a `bool` keeps its `false` half and
    // every nullable member keeps its `null`.
    assert_eq!(
        PhpType::parse("?int|?bool|?string|?DateTime")
            .falsy_type()
            .unwrap()
            .to_string(),
        "int|false|string|null"
    );
    // An object is truthy whatever it holds, so it is dropped outright.
    assert_eq!(
        PhpType::parse("DateTime|string")
            .falsy_type()
            .unwrap()
            .to_string(),
        "string"
    );
    // A union that is entirely truthy has no falsy half at all.
    assert_eq!(PhpType::parse("true|DateTime").falsy_type(), None);
    // Refinements PHP has no plain spelling for are left alone, exactly
    // as the truthy side leaves `int` rather than excluding `0`.
    assert_eq!(
        PhpType::parse("int").falsy_type().unwrap().to_string(),
        "int"
    );
}

#[test]
fn int_widens_to_float_for_compatibility_but_not_for_union_simplification() {
    // `int` accepts wherever `float` is expected (PHP's silent int→float
    // coercion), but the two remain distinct scalar domains: simplifying
    // `int|float` must not collapse it down to `float` the way a genuine
    // same-domain refinement (`positive-int|int` → `int`) does.
    assert_eq!(
        PhpType::parse("int|float").simplified().to_string(),
        "int|float"
    );
    assert_eq!(
        PhpType::parse("positive-int|float")
            .simplified()
            .to_string(),
        "positive-int|float"
    );
}
