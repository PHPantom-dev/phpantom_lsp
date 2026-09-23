use super::*;
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionInfo};

fn empty_class(name: &str) -> ClassInfo {
    ClassInfo {
        name: atom(name),
        ..ClassInfo::default()
    }
}

fn empty_function(name: &str) -> FunctionInfo {
    FunctionInfo {
        name: atom(name),
        name_offset: 0,
        parameters: Vec::new().into(),
        return_type: None,
        native_return_type: None,
        description: None,
        return_description: None,
        links: Vec::new(),
        see_refs: Vec::new(),
        namespace: None,
        conditional_return: None,
        type_assertions: Vec::new(),
        deprecation_message: None,
        deprecated_replacement: None,
        throws: Vec::new(),
        template_params: Vec::new(),
        template_param_bounds: Default::default(),
        template_bindings: Vec::new(),
        is_polyfill: false,
        overloads: Vec::new(),
        is_pure: false,
    }
}

#[test]
fn weak_map_gets_array_access_generics() {
    let mut class = empty_class("WeakMap");
    apply_class_stub_patches(&mut class);

    assert!(
        class
            .implements_generics
            .iter()
            .any(|(n, args)| n.as_str() == "ArrayAccess"
                && args.len() == 2
                && args[0] == PhpType::named(atom("TKey"))
                && args[1] == PhpType::named(atom("TValue"))),
        "Should have @implements ArrayAccess<TKey, TValue>"
    );
}

#[test]
fn unrelated_class_not_patched() {
    let mut class = empty_class("MyApp\\Foo");
    let original_params = class.template_params.clone();

    apply_class_stub_patches(&mut class);

    assert_eq!(class.template_params, original_params);
    assert!(class.implements_generics.is_empty());
}

#[test]
fn iterator_iterator_gets_templates_and_mixin() {
    let mut class = empty_class("IteratorIterator");
    apply_class_stub_patches(&mut class);

    assert_eq!(
        class.template_params,
        vec![atom("TKey"), atom("TValue"), atom("TIterator")]
    );
    assert!(
        class
            .implements_generics
            .iter()
            .any(|(n, args)| n.as_str() == "OuterIterator" && args.len() == 2),
        "Should have @implements OuterIterator<TKey, TValue>"
    );
    assert_eq!(class.mixins, vec![atom("TIterator")]);
    assert!(
        class.template_param_bounds.contains_key(&atom("TIterator")),
        "TIterator should have a bound"
    );
}

fn param(name: &str, type_hint: &str) -> crate::types::ParameterInfo {
    crate::types::ParameterInfo {
        name: atom(name),
        is_required: true,
        type_hint: Some(PhpType::parse(type_hint)),
        native_type_hint: Some(PhpType::parse(type_hint)),
        description: None,
        default_value: None,
        is_variadic: false,
        is_reference: false,
        closure_this_type: None,
    }
}

#[test]
fn array_map_links_callback_to_array_element() {
    let mut func = empty_function("array_map");
    func.parameters = vec![param("$callback", "callable"), param("$array", "array")].into();
    func.return_type = Some(PhpType::parse("array"));

    apply_function_stub_patches(&mut func);

    assert_eq!(func.template_params, vec![atom("TValue")]);
    assert_eq!(
        func.template_bindings,
        vec![(atom("TValue"), atom("$array"))]
    );
    // The callback's first parameter is now `TValue`.
    let callback = &func.parameters[0];
    assert_eq!(
        callback.type_hint,
        Some(PhpType::parse("callable(TValue): mixed"))
    );
    // The array is `array<TValue>`.
    assert_eq!(
        func.parameters[1].type_hint,
        Some(PhpType::parse("array<TValue>"))
    );
    // The return type is left bare so the value-inspecting element
    // logic in raw_type_inference.rs stays authoritative.
    assert_eq!(func.return_type, Some(PhpType::parse("array")));
}

#[test]
fn array_filter_links_callback_to_array_element() {
    let mut func = empty_function("array_filter");
    func.parameters = vec![param("$array", "array"), param("$callback", "callable")].into();

    apply_function_stub_patches(&mut func);

    assert_eq!(func.template_params, vec![atom("TValue")]);
    assert_eq!(
        func.template_bindings,
        vec![(atom("TValue"), atom("$array"))]
    );
    assert_eq!(
        func.parameters[0].type_hint,
        Some(PhpType::parse("array<TValue>"))
    );
    assert_eq!(
        func.parameters[1].type_hint,
        Some(PhpType::parse("callable(TValue): mixed"))
    );
}

#[test]
fn array_map_unexpected_shape_not_patched() {
    // A hand-written `@method array_map(...)` or a differently-shaped
    // stub must not be rewritten.
    let mut func = empty_function("array_map");
    func.parameters = vec![param("$other", "array")].into();

    apply_function_stub_patches(&mut func);

    assert!(func.template_params.is_empty());
    assert!(func.template_bindings.is_empty());
}

#[test]
fn range_gets_conditional_return() {
    let mut func = empty_function("range");
    apply_function_stub_patches(&mut func);
    assert!(
        func.conditional_return.is_some(),
        "range() should have a conditional return type after patching"
    );
}

#[test]
fn str_word_count_gets_conditional_return() {
    let mut func = empty_function("str_word_count");
    apply_function_stub_patches(&mut func);
    let cond = func
        .conditional_return
        .expect("str_word_count() should have a conditional return type after patching");
    assert_eq!(
        cond.to_string(),
        "$format is 0 ? int : $format is 1 ? list<string> : \
         $format is 2 ? array<int, string> : list<string>|int"
    );
}

#[test]
fn stream_bucket_make_writeable_pre_84_becomes_stdclass() {
    let mut func = empty_function("stream_bucket_make_writeable");
    func.return_type = Some(PhpType::parse("object|null"));
    func.native_return_type = Some(PhpType::parse("object|null"));

    apply_function_stub_patches(&mut func);

    assert_eq!(func.return_type, Some(PhpType::parse("stdClass|null")));
    assert_eq!(
        func.native_return_type,
        Some(PhpType::parse("stdClass|null"))
    );
}

#[test]
fn stream_bucket_make_writeable_84_plus_unchanged() {
    let mut func = empty_function("stream_bucket_make_writeable");
    func.return_type = Some(PhpType::parse("StreamBucket|null"));
    func.native_return_type = Some(PhpType::parse("StreamBucket|null"));

    apply_function_stub_patches(&mut func);

    assert_eq!(func.return_type, Some(PhpType::parse("StreamBucket|null")));
    assert_eq!(
        func.native_return_type,
        Some(PhpType::parse("StreamBucket|null"))
    );
}

#[test]
fn ctype_family_and_define_accept_mixed() {
    for name in ["ctype_digit", "ctype_alpha", "ctype_xdigit"] {
        let mut func = empty_function(name);
        func.parameters = vec![param("$text", "string")].into();

        apply_function_stub_patches(&mut func);

        assert_eq!(
            func.parameters[0].type_hint,
            Some(PhpType::mixed()),
            "{name} should accept mixed"
        );
        assert_eq!(func.parameters[0].native_type_hint, Some(PhpType::mixed()));
    }

    let mut define = empty_function("define");
    define.parameters = vec![
        param("$constant_name", "string"),
        param("$value", "null|array|bool|int|float|string"),
    ]
    .into();

    apply_function_stub_patches(&mut define);

    assert_eq!(
        define.parameters[0].type_hint,
        Some(PhpType::string()),
        "the constant name is still a string"
    );
    assert_eq!(define.parameters[1].type_hint, Some(PhpType::mixed()));
}

/// Each conditional is keyed on the parameter that really decides the
/// return type, so a call that omits the argument is answered by that
/// parameter's declared default rather than by argument position.
#[test]
fn argument_dependent_builtins_get_conditional_returns() {
    /// A stub's name, its `(parameter, type)` list, and the conditional
    /// return type the patch is expected to give it.
    type PatchCase = (
        &'static str,
        &'static [(&'static str, &'static str)],
        &'static str,
    );

    let cases: &[PatchCase] = &[
        (
            "pathinfo",
            &[("$path", "string"), ("$flags", "int")],
            "$flags is 15 ? array{dirname: string, basename: string, \
             extension?: string, filename: string} : string",
        ),
        (
            "print_r",
            &[("$value", "mixed"), ("$return", "bool")],
            "$return is true ? string : true",
        ),
        (
            "hrtime",
            &[("$as_number", "bool")],
            "$as_number is true ? int|float : array{int, int}|false",
        ),
        (
            "microtime",
            &[("$as_float", "bool")],
            "$as_float is true ? float : string",
        ),
        (
            "getenv",
            &[("$name", "string"), ("$local_only", "bool")],
            "$name is null ? array<string, string> : string|false",
        ),
        (
            "mb_convert_encoding",
            &[("$string", "array|string"), ("$to_encoding", "string")],
            "$string is array ? array<array-key, string> : string|false",
        ),
        ("abs", &[("$num", "int|float")], "$num is int ? int : float"),
    ];

    for (name, params, expected) in cases {
        let mut func = empty_function(name);
        func.parameters = params
            .iter()
            .map(|(n, t)| param(n, t))
            .collect::<Vec<_>>()
            .into();

        apply_function_stub_patches(&mut func);

        let cond = func
            .conditional_return
            .unwrap_or_else(|| panic!("{name} should have a conditional return type"));
        assert_eq!(cond.to_string(), *expected, "{name}");
    }
}

/// The pre-7.1 `$varname` spelling is what a call binds to on an older
/// configured PHP version, so the conditional follows the first parameter
/// rather than a hard-coded name.
#[test]
fn getenv_keys_on_whichever_name_parameter_the_stub_declares() {
    let mut func = empty_function("getenv");
    func.parameters = vec![param("$varname", "string"), param("$local_only", "bool")].into();

    apply_function_stub_patches(&mut func);

    assert_eq!(
        func.conditional_return.map(|c| c.to_string()).as_deref(),
        Some("$varname is null ? array<string, string> : string|false")
    );
}

/// A stub that does not declare the parameter the conditional keys on is
/// left alone: deciding the branch against whichever argument landed in
/// slot 0 is worse than the declared union.
#[test]
fn a_differently_shaped_stub_keeps_its_declared_return() {
    let mut func = empty_function("pathinfo");
    func.parameters = vec![param("$path", "string")].into();

    apply_function_stub_patches(&mut func);

    assert!(func.conditional_return.is_none());
}

#[test]
fn simple_xml_serialisers_get_conditional_returns() {
    let mut class = empty_class("SimpleXMLElement");
    for name in ["asXML", "saveXML"] {
        let mut method = crate::types::MethodInfo::virtual_method(name, Some("string|bool"));
        method.parameters = vec![param("$filename", "string|null")].into();
        class.methods.make_mut().push(std::sync::Arc::new(method));
    }

    apply_class_stub_patches(&mut class);

    for method in class.methods.iter() {
        assert_eq!(
            method.conditional_return.as_ref().map(|c| c.to_string()),
            Some("$filename is null ? string|false : bool".to_string()),
            "{}",
            method.name
        );
    }
}

#[test]
fn reflection_class_new_instance_args_loses_its_null_branch() {
    let mut class = empty_class("ReflectionClass");
    let mut method = crate::types::MethodInfo::virtual_method("newInstanceArgs", Some("T|null"));
    method.native_return_type = Some(PhpType::parse("?object"));
    class.methods.make_mut().push(std::sync::Arc::new(method));

    apply_class_stub_patches(&mut class);

    assert_eq!(class.methods[0].return_type, Some(PhpType::parse("T")));
    assert_eq!(
        class.methods[0].native_return_type,
        Some(PhpType::parse("object"))
    );
}

#[test]
fn reflection_class_leaves_an_already_non_null_return_alone() {
    let mut class = empty_class("ReflectionClass");
    let method = crate::types::MethodInfo::virtual_method("newInstanceArgs", Some("T"));
    class.methods.make_mut().push(std::sync::Arc::new(method));

    apply_class_stub_patches(&mut class);

    assert_eq!(class.methods[0].return_type, Some(PhpType::parse("T")));
}

#[test]
fn spl_autoload_register_types_its_callback_parameter() {
    let mut func = empty_function("spl_autoload_register");
    func.parameters = vec![param("$callback", "?callable"), param("$throw", "bool")].into();

    apply_function_stub_patches(&mut func);

    assert_eq!(
        func.parameters[0].type_hint,
        Some(PhpType::parse("?callable(string): void"))
    );
    assert_eq!(func.parameters[1].type_hint, Some(PhpType::parse("bool")));
}

#[test]
fn spl_autoload_register_unexpected_shape_not_patched() {
    let mut func = empty_function("spl_autoload_register");
    func.parameters = vec![param("$other", "?callable")].into();

    apply_function_stub_patches(&mut func);

    assert_eq!(
        func.parameters[0].type_hint,
        Some(PhpType::parse("?callable"))
    );
}

#[test]
fn the_user_sort_family_types_its_comparison_callback() {
    for (name, compared) in [
        ("usort", "TValue"),
        ("uasort", "TValue"),
        ("uksort", "TKey"),
    ] {
        let mut func = empty_function(name);
        func.parameters = vec![param("$array", "array"), param("$callback", "callable")].into();

        apply_function_stub_patches(&mut func);

        assert_eq!(
            func.parameters[0].type_hint,
            Some(PhpType::parse("array<TKey, TValue>")),
            "{name} array parameter"
        );
        assert_eq!(
            func.parameters[1].type_hint,
            Some(PhpType::parse(&format!(
                "callable({compared}, {compared}): int"
            ))),
            "{name} callback parameter"
        );
        assert_eq!(
            func.template_bindings,
            vec![
                (atom("TKey"), atom("$array")),
                (atom("TValue"), atom("$array")),
            ],
            "{name} bindings"
        );
    }
}

#[test]
fn a_differently_shaped_sort_stub_is_not_patched() {
    let mut func = empty_function("usort");
    func.parameters = vec![param("$callback", "callable"), param("$array", "array")].into();

    apply_function_stub_patches(&mut func);

    assert!(func.template_params.is_empty());
    assert_eq!(
        func.parameters[0].type_hint,
        Some(PhpType::parse("callable"))
    );
}

#[test]
fn an_unrelated_function_keeps_its_parameter_types() {
    let mut func = empty_function("str_pad");
    func.parameters = vec![param("$string", "string")].into();

    apply_function_stub_patches(&mut func);

    assert_eq!(func.parameters[0].type_hint, Some(PhpType::string()));
}
