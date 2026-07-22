use super::*;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::test_fixtures::{make_class, make_constant, make_method, make_property, no_loader};
use std::sync::Arc;

// ── applies_to ──────────────────────────────────────────────────────

#[test]
fn applies_when_docblock_present() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.class_docblock = Some("/** @method void bar() */".to_string());
    assert!(provider.applies_to(&class, &no_loader));
}

#[test]
fn does_not_apply_when_no_docblock_and_no_mixins() {
    let provider = PHPDocProvider;
    let class = make_class("Foo");
    assert!(!provider.applies_to(&class, &no_loader));
}

#[test]
fn does_not_apply_when_docblock_empty_and_no_mixins() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.class_docblock = Some(String::new());
    assert!(!provider.applies_to(&class, &no_loader));
}

#[test]
fn applies_when_class_has_mixins() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];

    let class_loader = |_: &str| -> Option<Arc<ClassInfo>> { None };
    assert!(provider.applies_to(&class, &class_loader));
}

#[test]
fn applies_when_ancestor_has_mixins() {
    let provider = PHPDocProvider;
    let mut class = make_class("Child");
    class.parent_class = Some(atom("Parent"));

    let mut parent = make_class("Parent");
    parent.mixins = vec![atom("Mixin")];

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Parent" {
            Some(Arc::new(parent.clone()))
        } else {
            None
        }
    };
    assert!(provider.applies_to(&class, &class_loader));
}

// ── provide: @method ────────────────────────────────────────────────

#[test]
fn provides_method_tags() {
    let provider = PHPDocProvider;
    let mut class = make_class("Cart");
    class.class_docblock = Some(
        concat!(
            "/**\n",
            " * @method string getName()\n",
            " * @method void setName(string $name)\n",
            " */",
        )
        .to_string(),
    );

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.methods.len(), 2);
    assert!(result.methods.iter().any(|m| m.name == "getName"));
    assert!(result.methods.iter().any(|m| m.name == "setName"));
}

#[test]
fn provides_static_method_tags() {
    let provider = PHPDocProvider;
    let mut class = make_class("Facade");
    class.class_docblock =
        Some(concat!("/**\n", " * @method static int count()\n", " */",).to_string());

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.methods.len(), 1);
    assert!(result.methods[0].is_static);
    assert_eq!(result.methods[0].name, "count");
    assert_eq!(result.methods[0].return_type_str().as_deref(), Some("int"));
}

#[test]
fn method_tag_preserves_return_type() {
    let provider = PHPDocProvider;
    let mut class = make_class("TestCase");
    class.class_docblock = Some(
        concat!(
            "/**\n",
            " * @method \\Mockery\\MockInterface mock(string $abstract)\n",
            " */",
        )
        .to_string(),
    );

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.methods.len(), 1);
    assert_eq!(
        result.methods[0].return_type_str().as_deref(),
        Some("\\Mockery\\MockInterface")
    );
}

#[test]
fn method_tag_parses_parameters() {
    let provider = PHPDocProvider;
    let mut class = make_class("DB");
    class.class_docblock = Some(concat!(
        "/**\n",
        " * @method void assertDatabaseHas(string $table, array $data, string $connection = null)\n",
        " */",
    ).to_string());

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.methods.len(), 1);
    let method = &result.methods[0];
    assert_eq!(method.parameters.len(), 3);
    assert!(method.parameters[0].is_required);
    assert!(method.parameters[1].is_required);
    assert!(!method.parameters[2].is_required, "$connection has default");
}

// ── provide: @property ──────────────────────────────────────────────

#[test]
fn provides_property_tags() {
    let provider = PHPDocProvider;
    let mut class = make_class("Customer");
    class.class_docblock = Some(
        concat!(
            "/**\n",
            " * @property int $id\n",
            " * @property string $name\n",
            " */",
        )
        .to_string(),
    );

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.properties.len(), 2);
    assert!(result.properties.iter().any(|p| p.name == "id"));
    assert!(result.properties.iter().any(|p| p.name == "name"));
}

#[test]
fn provides_property_read_and_write_tags() {
    let provider = PHPDocProvider;
    let mut class = make_class("Controller");
    class.class_docblock = Some(
        concat!(
            "/**\n",
            " * @property-read Session $session\n",
            " * @property-write string $title\n",
            " */",
        )
        .to_string(),
    );

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.properties.len(), 2);
    let session = result
        .properties
        .iter()
        .find(|p| p.name == "session")
        .unwrap();
    assert_eq!(session.type_hint_str().as_deref(), Some("Session"));
    let title = result
        .properties
        .iter()
        .find(|p| p.name == "title")
        .unwrap();
    assert_eq!(title.type_hint_str().as_deref(), Some("string"));
}

#[test]
fn property_tags_are_public_and_non_static() {
    let provider = PHPDocProvider;
    let mut class = make_class("Model");
    class.class_docblock = Some("/** @property int $id */".to_string());

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.properties.len(), 1);
    assert_eq!(result.properties[0].visibility, Visibility::Public);
    assert!(!result.properties[0].is_static);
}

#[test]
fn nullable_type_preserved() {
    let provider = PHPDocProvider;
    let mut class = make_class("Customer");
    class.class_docblock = Some("/** @property null|int $agreement_id */".to_string());

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.properties.len(), 1);
    assert_eq!(
        result.properties[0].type_hint_str().as_deref(),
        Some("null|int"),
        "null|int should be preserved in the type hint"
    );
}

// ── provide: no constants from tags ─────────────────────────────────

#[test]
fn tags_never_produce_constants() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.class_docblock = Some(
        concat!(
            "/**\n",
            " * @method void bar()\n",
            " * @property int $baz\n",
            " */",
        )
        .to_string(),
    );

    let result = provider.provide(&class, &no_loader, None);
    assert!(result.constants.is_empty());
}

// ── provide: empty / missing docblock ───────────────────────────────

#[test]
fn empty_docblock_returns_empty() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.class_docblock = Some("/** */".to_string());

    let result = provider.provide(&class, &no_loader, None);
    assert!(result.methods.is_empty());
    assert!(result.properties.is_empty());
    assert!(result.constants.is_empty());
}

#[test]
fn no_docblock_returns_empty() {
    let provider = PHPDocProvider;
    let class = make_class("Foo");

    let result = provider.provide(&class, &no_loader, None);
    assert!(result.is_empty());
}

// ── provide: mixed @method and @property tags ───────────────────────

#[test]
fn provides_both_methods_and_properties() {
    let provider = PHPDocProvider;
    let mut class = make_class("Model");
    class.class_docblock = Some(
        concat!(
            "/**\n",
            " * @property string $name\n",
            " * @method static Model find(int $id)\n",
            " * @property-read int $id\n",
            " * @method void save()\n",
            " */",
        )
        .to_string(),
    );

    let result = provider.provide(&class, &no_loader, None);
    assert_eq!(result.methods.len(), 2);
    assert_eq!(result.properties.len(), 2);
}

// ── provide: @mixin members ─────────────────────────────────────────

#[test]
fn provides_public_methods_from_mixin() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.methods
        .push(Arc::new(make_method("doStuff", Some("string"))));
    let mut private_method = make_method("secret", Some("void"));
    private_method.visibility = Visibility::Private;
    bar.methods.push(Arc::new(private_method));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 1);
    assert_eq!(result.methods[0].name, "doStuff");
}

#[test]
fn provides_public_properties_from_mixin() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.properties.push(make_property("name", Some("string")));
    let mut protected_prop = make_property("internal", Some("int"));
    protected_prop.visibility = Visibility::Protected;
    bar.properties.push(protected_prop);

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.properties.len(), 1);
    assert_eq!(result.properties[0].name, "name");
}

#[test]
fn provides_public_constants_from_mixin() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.constants.push(make_constant("MAX_SIZE"));
    let mut private_const = make_constant("INTERNAL");
    private_const.visibility = Visibility::Private;
    bar.constants.push(private_const);

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.constants.len(), 1);
    assert_eq!(result.constants[0].name, "MAX_SIZE");
}

#[test]
fn mixin_does_not_overwrite_existing_class_members() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];
    class
        .methods
        .push(Arc::new(make_method("doStuff", Some("int"))));

    let mut bar = make_class("Bar");
    bar.methods
        .push(Arc::new(make_method("doStuff", Some("string"))));
    bar.methods
        .push(Arc::new(make_method("barOnly", Some("void"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    // "doStuff" is already on the class, so only "barOnly" should appear
    assert_eq!(result.methods.len(), 1);
    assert_eq!(result.methods[0].name, "barOnly");
}

#[test]
fn mixin_leaves_this_return_type_as_is_for_consumer_resolution() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.methods
        .push(Arc::new(make_method("fluent", Some("$this"))));
    bar.methods
        .push(Arc::new(make_method("selfRef", Some("self"))));
    bar.methods
        .push(Arc::new(make_method("staticRef", Some("static"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 3);
    // Return types are left as-is so that $this/self/static resolve
    // to the consuming class when the method is called on it.
    let expected = [
        ("fluent", "$this"),
        ("selfRef", "self"),
        ("staticRef", "static"),
    ];
    for (name, expected_ret) in &expected {
        let method = result.methods.iter().find(|m| m.name == *name).unwrap();
        assert_eq!(
            method.return_type_str().as_deref(),
            Some(*expected_ret),
            "method '{}' should keep its original return type for consumer resolution",
            name
        );
    }
}

#[test]
fn forwards_calls_rewrites_mixin_class_return_to_this() {
    let provider = PHPDocProvider;
    let mut class = make_class("Illuminate\\Database\\Eloquent\\Builder");
    class.mixins = vec![atom("Illuminate\\Database\\Query\\Builder")];
    class.used_traits = vec![atom("Illuminate\\Support\\Traits\\ForwardsCalls")];

    let mut query = make_class("Illuminate\\Database\\Query\\Builder");
    query
        .methods
        .push(Arc::new(make_method("lockForUpdate", Some("$this"))));
    query.methods.push(Arc::new(make_method(
        "newQuery",
        Some("Illuminate\\Database\\Query\\Builder"),
    )));
    query
        .methods
        .push(Arc::new(make_method("count", Some("int"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Illuminate\\Database\\Query\\Builder"
            || name.ends_with("\\Query\\Builder")
            || name == "Builder"
        {
            Some(Arc::new(query.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    let lock = result
        .methods
        .iter()
        .find(|m| m.name == "lockForUpdate")
        .expect("lockForUpdate");
    assert_eq!(lock.return_type_str().as_deref(), Some("$this"));

    let new_query = result
        .methods
        .iter()
        .find(|m| m.name == "newQuery")
        .expect("newQuery");
    assert_eq!(
        new_query.return_type_str().as_deref(),
        Some("$this"),
        "explicit mixin-class return should rewrite to $this under ForwardsCalls"
    );

    let count = result
        .methods
        .iter()
        .find(|m| m.name == "count")
        .expect("count");
    assert_eq!(
        count.return_type_str().as_deref(),
        Some("int"),
        "non-self returns must pass through"
    );
}

#[test]
fn mixin_collects_from_ancestor_mixins() {
    let provider = PHPDocProvider;
    let mut class = make_class("Child");
    class.parent_class = Some(atom("Parent"));

    let mut parent = make_class("Parent");
    parent.mixins = vec![atom("Mixin")];

    let mut mixin = make_class("Mixin");
    mixin
        .methods
        .push(Arc::new(make_method("mixinMethod", Some("void"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Parent" => Some(Arc::new(parent.clone())),
            "Mixin" => Some(Arc::new(mixin.clone())),
            _ => None,
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 1);
    assert_eq!(result.methods[0].name, "mixinMethod");
}

#[test]
fn mixin_recurses_into_mixin_mixins() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.mixins = vec![atom("Baz")];
    bar.methods
        .push(Arc::new(make_method("barMethod", Some("void"))));

    let mut baz = make_class("Baz");
    baz.methods
        .push(Arc::new(make_method("bazMethod", Some("void"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Bar" => Some(Arc::new(bar.clone())),
            "Baz" => Some(Arc::new(baz.clone())),
            _ => None,
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 2);
    assert!(result.methods.iter().any(|m| m.name == "barMethod"));
    assert!(result.methods.iter().any(|m| m.name == "bazMethod"));
}

#[test]
fn multiple_mixins() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar"), atom("Baz")];

    let mut bar = make_class("Bar");
    bar.methods
        .push(Arc::new(make_method("barMethod", Some("void"))));

    let mut baz = make_class("Baz");
    baz.methods
        .push(Arc::new(make_method("bazMethod", Some("void"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Bar" => Some(Arc::new(bar.clone())),
            "Baz" => Some(Arc::new(baz.clone())),
            _ => None,
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 2);
    assert!(result.methods.iter().any(|m| m.name == "barMethod"));
    assert!(result.methods.iter().any(|m| m.name == "bazMethod"));
}

#[test]
fn first_mixin_wins_on_name_collision() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar"), atom("Baz")];

    let mut bar = make_class("Bar");
    bar.methods
        .push(Arc::new(make_method("shared", Some("string"))));

    let mut baz = make_class("Baz");
    baz.methods
        .push(Arc::new(make_method("shared", Some("int"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Bar" => Some(Arc::new(bar.clone())),
            "Baz" => Some(Arc::new(baz.clone())),
            _ => None,
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 1);
    assert_eq!(
        result.methods[0].return_type_str().as_deref(),
        Some("string"),
        "first mixin should win"
    );
}

// ── @method / @property tags take precedence over @mixin ────────────

#[test]
fn method_tag_beats_mixin_method() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.class_docblock = Some("/** @method string doStuff() */".to_string());
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.methods
        .push(Arc::new(make_method("doStuff", Some("int"))));
    bar.methods
        .push(Arc::new(make_method("barOnly", Some("void"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 2);
    let do_stuff = result.methods.iter().find(|m| m.name == "doStuff").unwrap();
    assert_eq!(
        do_stuff.return_type_str().as_deref(),
        Some("string"),
        "@method tag should take precedence over mixin method"
    );
    assert!(
        result.methods.iter().any(|m| m.name == "barOnly"),
        "non-conflicting mixin method should still appear"
    );
}

#[test]
fn property_tag_beats_mixin_property() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.class_docblock = Some("/** @property string $name */".to_string());
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.properties.push(make_property("name", Some("int")));
    bar.properties.push(make_property("email", Some("string")));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.properties.len(), 2);
    let name = result.properties.iter().find(|p| p.name == "name").unwrap();
    assert_eq!(
        name.type_hint_str().as_deref(),
        Some("string"),
        "@property tag should take precedence over mixin property"
    );
    assert!(
        result.properties.iter().any(|p| p.name == "email"),
        "non-conflicting mixin property should still appear"
    );
}

#[test]
fn mixin_only_no_docblock() {
    let provider = PHPDocProvider;
    let mut class = make_class("Foo");
    class.mixins = vec![atom("Bar")];

    let mut bar = make_class("Bar");
    bar.methods
        .push(Arc::new(make_method("barMethod", Some("void"))));
    bar.properties.push(make_property("barProp", Some("int")));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Bar" {
            Some(Arc::new(bar.clone()))
        } else {
            None
        }
    };

    let result = provider.provide(&class, &class_loader, None);
    assert_eq!(result.methods.len(), 1);
    assert_eq!(result.methods[0].name, "barMethod");
    assert_eq!(result.properties.len(), 1);
    assert_eq!(result.properties[0].name, "barProp");
}

/// When a class declares `@template TWraps` and `@mixin TWraps`, and a
/// child class extends it with a concrete type (`extends Subclient<EventsApi>`),
/// the mixin name should be resolved through the template substitution map
/// so that `EventsApi`'s public members appear on the child class.
#[test]
fn mixin_template_param_substituted_via_ancestor_walk() {
    clear_mixin_cache();

    let provider = PHPDocProvider;

    // The child class: extends Subclient<EventsApi>
    let mut child = make_class("EventSubclient");
    child.parent_class = Some(atom("Subclient"));
    child.extends_generics = vec![(
        atom("Subclient"),
        vec![PhpType::Named("EventsApi".to_string())],
    )];

    // The parent class: @template TWraps, @mixin TWraps
    let mut subclient = make_class("Subclient");
    subclient.template_params = vec![atom("TWraps")];
    subclient.mixins = vec![atom("TWraps")];

    // The mixin target class
    let mut events_api = make_class("EventsApi");
    events_api
        .methods
        .push(Arc::new(make_method("createEvent", Some("void"))));
    events_api
        .methods
        .push(Arc::new(make_method("getEvents", Some("array"))));

    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Subclient" => Some(Arc::new(subclient.clone())),
            "EventsApi" => Some(Arc::new(events_api.clone())),
            _ => None,
        }
    };

    let result = provider.provide(&child, &class_loader, None);

    let method_names: Vec<&str> = result.methods.iter().map(|m| m.name.as_str()).collect();
    assert!(
        method_names.contains(&"createEvent"),
        "Expected createEvent from mixin TWraps→EventsApi, got: {method_names:?}"
    );
    assert!(
        method_names.contains(&"getEvents"),
        "Expected getEvents from mixin TWraps→EventsApi, got: {method_names:?}"
    );
}
