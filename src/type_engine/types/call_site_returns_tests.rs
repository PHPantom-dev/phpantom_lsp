use super::*;
use crate::atom::atom;

fn params(names: &[&str]) -> Vec<ParameterInfo> {
    names
        .iter()
        .enumerate()
        .map(|(i, name)| ParameterInfo {
            name: atom(name),
            is_required: i == 0,
            type_hint: None,
            native_type_hint: None,
            description: None,
            default_value: None,
            is_variadic: false,
            is_reference: false,
            closure_this_type: None,
            param_out_type: None,
            callable_invocation: None,
        })
        .collect()
}

fn string_or_false() -> PhpType {
    PhpType::union(vec![PhpType::string(), PhpType::false_()])
}

fn narrow(text_args: &str) -> Option<String> {
    call_site_return_type(
        "json_encode",
        &params(&["$value", "$flags", "$depth"]),
        text_args,
        &string_or_false(),
        None,
    )
    .map(|ty| ty.to_string())
}

/// Resolves the handful of argument texts the filter tests pass as typed
/// expressions.
fn filter_resolver(text: &str) -> Option<PhpType> {
    let parse = |s: &str| crate::php_type::PhpType::parse(s);
    match text {
        "$intOptions" => Some(parse("array{options: array{min_range: 1}}")),
        "$boundedOptions" => Some(parse(
            "array{options: array{min_range: 1, max_range: 10, default: 5}}",
        )),
        "$nullFlags" => Some(parse("array{flags: 134217728}")),
        "$opaque" => Some(PhpType::array()),
        "self::FILTER" => Some(PhpType::literal_int("257")),
        _ => None,
    }
}

fn filter_var(text_args: &str) -> Option<String> {
    call_site_return_type(
        "filter_var",
        &params(&["$value", "$filter", "$options"]),
        text_args,
        &PhpType::mixed(),
        Some(&filter_resolver),
    )
    .map(|ty| ty.to_string())
}

fn class_implements(text_args: &str) -> Option<PhpType> {
    let resolver = |text: &str| match text {
        "$object" => Some(PhpType::named(atom("Foo"))),
        "$class" => Some(PhpType::class_string(Some(PhpType::named(atom("Foo"))))),
        "$either" => Some(PhpType::union(vec![
            PhpType::named(atom("Foo")),
            PhpType::class_string(None),
        ])),
        "$string" => Some(PhpType::string()),
        _ => None,
    };
    call_site_return_type(
        "class_implements",
        &params(&["$object_or_class", "$autoload"]),
        text_args,
        &PhpType::union(vec![PhpType::array(), PhpType::false_()]),
        Some(&resolver),
    )
}

#[test]
fn named_flag_drops_the_false_branch() {
    assert_eq!(
        narrow("$value, JSON_THROW_ON_ERROR").as_deref(),
        Some("string")
    );
}

#[test]
fn flag_is_found_beside_other_flags() {
    assert_eq!(
        narrow("$value, JSON_PRETTY_PRINT | \\JSON_THROW_ON_ERROR").as_deref(),
        Some("string")
    );
}

#[test]
fn numeric_mask_is_bit_tested() {
    assert_eq!(narrow("$value, 4194432").as_deref(), Some("string"));
    assert_eq!(narrow("$value, 128").as_deref(), None);
}

#[test]
fn named_argument_is_bound_to_the_flags_parameter() {
    assert_eq!(
        narrow("$value, depth: 8, flags: JSON_THROW_ON_ERROR").as_deref(),
        Some("string")
    );
}

#[test]
fn unknown_mask_leaves_the_declared_union() {
    assert_eq!(narrow("$value, $flags").as_deref(), None);
    assert_eq!(narrow("$value").as_deref(), None);
}

#[test]
fn other_json_flags_leave_the_declared_union() {
    assert_eq!(narrow("$value, JSON_UNESCAPED_SLASHES").as_deref(), None);
}

#[test]
fn resolved_literal_int_argument_is_bit_tested() {
    let resolver = |text: &str| match text {
        "self::FLAGS" => Some(PhpType::literal_int("4194304")),
        _ => None,
    };
    let narrowed = call_site_return_type(
        "json_encode",
        &params(&["$value", "$flags", "$depth"]),
        "$value, self::FLAGS",
        &string_or_false(),
        Some(&resolver),
    );
    assert_eq!(narrowed.map(|ty| ty.to_string()).as_deref(), Some("string"));
}

#[test]
fn a_boolean_or_is_not_read_as_a_mask() {
    assert_eq!(split_bitwise_or("$a || JSON_THROW_ON_ERROR").len(), 1);
}

#[test]
fn a_pipe_in_a_string_does_not_split_a_mask() {
    assert_eq!(
        split_bitwise_or("f('a|b') | JSON_THROW_ON_ERROR"),
        vec!["f('a|b') ", " JSON_THROW_ON_ERROR"]
    );
}

#[test]
fn filter_var_without_a_filter_is_a_string_or_false() {
    assert_eq!(filter_var("$v").as_deref(), Some("string|false"));
}

#[test]
fn validation_filters_decide_the_result() {
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT").as_deref(),
        Some("int|false")
    );
    assert_eq!(
        filter_var("$v, \\FILTER_VALIDATE_FLOAT").as_deref(),
        Some("float|false")
    );
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_EMAIL").as_deref(),
        Some("non-falsy-string|false")
    );
    assert_eq!(filter_var("$v, self::FILTER").as_deref(), Some("int|false"));
}

#[test]
fn bool_filter_folds_its_failure_into_bool() {
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_BOOL").as_deref(),
        Some("bool")
    );
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_BOOLEAN, FILTER_NULL_ON_FAILURE").as_deref(),
        Some("bool|null")
    );
}

#[test]
fn null_on_failure_swaps_false_for_null() {
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT, FILTER_FLAG_ALLOW_HEX | FILTER_NULL_ON_FAILURE")
            .as_deref(),
        Some("int|null")
    );
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_FLOAT, $nullFlags").as_deref(),
        Some("float|null")
    );
}

#[test]
fn int_range_options_bound_the_result() {
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT, $intOptions").as_deref(),
        Some("int<1, max>|false")
    );
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT, $boundedOptions").as_deref(),
        Some("int<1, 10>|5")
    );
}

#[test]
fn throw_on_failure_leaves_only_the_filtered_type() {
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT, FILTER_THROW_ON_FAILURE").as_deref(),
        Some("int")
    );
}

#[test]
fn undecidable_filter_calls_keep_the_declared_type() {
    assert_eq!(filter_var("$v, $filter").as_deref(), None);
    assert_eq!(filter_var("$v, FILTER_CALLBACK").as_deref(), None);
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT, FILTER_REQUIRE_ARRAY").as_deref(),
        None
    );
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT, FILTER_FORCE_ARRAY").as_deref(),
        None
    );
    assert_eq!(
        filter_var("$v, FILTER_VALIDATE_INT, $opaque").as_deref(),
        None
    );
}

#[test]
fn filter_input_adds_the_missing_variable_result() {
    let ty = call_site_return_type(
        "filter_input",
        &params(&["$type", "$var_name", "$filter", "$options"]),
        "INPUT_GET, 'id', FILTER_VALIDATE_INT",
        &PhpType::mixed(),
        None,
    );
    assert_eq!(ty.map(|t| t.to_string()).as_deref(), Some("int|false|null"));
}

#[test]
fn class_implements_on_an_object_cannot_fail() {
    let ty = class_implements("$object").unwrap();
    assert_eq!(ty.to_string(), "array");
}

#[test]
fn class_implements_on_a_class_string_fails_benevolently() {
    for args in ["$class", "$either", "$class, true"] {
        let ty = class_implements(args).unwrap();
        assert!(ty.is_benevolent(), "{args}");
        assert_eq!(ty.strip_benevolence().to_string(), "array|false");
    }
}

#[test]
fn class_implements_keeps_false_when_it_can_happen() {
    assert!(class_implements("$string").is_none());
    assert!(class_implements("$class, false").is_none());
    assert!(class_implements("$unknown").is_none());
}
