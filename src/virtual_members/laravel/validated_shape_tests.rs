use super::*;
use crate::class_lookup::Fqn;
use crate::test_fixtures::make_class;
use crate::types::ClassLikeKind;
use crate::virtual_members::laravel::validation_rules::rules_from_array_text;

/// The enums the enum-rule tests name, and nothing else.
///
/// Names are the ones written in the rules arrays below, which are parsed
/// standalone and so keep their short form.
fn test_enum(name: Fqn<'_>) -> Option<Arc<ClassInfo>> {
    let name = name.as_str();
    let backed = match name {
        "Role" | "JamFlavor" => Some(BackedEnumType::String),
        "Level" | "BatchSize" => Some(BackedEnumType::Int),
        // A pure enum: no backing type, and so no raw scalar form.
        "Suit" => None,
        _ => return None,
    };
    let mut class = make_class(name);
    class.kind = ClassLikeKind::Enum;
    class.backed_type = backed;
    Some(Arc::new(class))
}

/// Build the shape for a rules array written as PHP source, and render it.
fn shape_of(array_text: &str) -> String {
    let source = "<?php use Illuminate\\Validation\\Rule; use Illuminate\\Validation\\Rules\\Enum;";
    let rules = rules_from_array_text_in_context(array_text, source, source.len() as u32)
        .expect("rules should parse");
    rules_to_shape(&rules, &test_enum)
        .map(|ty| ty.to_string())
        .unwrap_or_else(|| "array".to_string())
}

#[test]
fn required_string_is_a_required_key() {
    assert_eq!(
        shape_of("['name' => 'required|string|max:255']"),
        "array{name: string}"
    );
}

#[test]
fn nullable_adds_null_and_leaves_the_key_optional() {
    // Validating `['age' => 'nullable|integer']` against `[]` yields `[]`,
    // not `['age' => null]`, so the key is optional as well as nullable.
    assert_eq!(
        shape_of("['age' => 'nullable|integer']"),
        "array{age?: ?int}"
    );
}

/// `exclude` validates the field and then drops it, so it is never a key of
/// the validated array.
#[test]
fn an_excluded_field_is_not_part_of_the_shape() {
    assert_eq!(
        shape_of("['name' => 'required|string', 'token' => 'exclude|string']"),
        "array{name: string}"
    );
    // Nothing left to describe once the only field is excluded.
    assert_eq!(shape_of("['token' => 'exclude|string']"), "array");
    // A dotted subtree goes with its excluded root.
    assert_eq!(
        shape_of(
            "['name' => 'required|string', 'meta' => 'exclude|array', 'meta.tag' => 'string']"
        ),
        "array{name: string}"
    );
}

/// The conditional family only sometimes drops the field, so the key stays but
/// may be missing.
#[test]
fn a_conditionally_excluded_field_is_an_optional_key() {
    for rule in [
        "exclude_if:other,1",
        "exclude_unless:other,1",
        "exclude_with:other",
        "exclude_without:other",
    ] {
        assert_eq!(
            shape_of(&format!("['name' => 'required|string|{rule}']")),
            "array{name?: string}",
            "{rule} should leave an optional key"
        );
    }
}

#[test]
fn present_guarantees_the_key_without_requiring_a_value() {
    assert_eq!(
        shape_of("['note' => 'present|string']"),
        "array{note: string}"
    );
}

#[test]
fn a_conditional_requirement_still_allows_the_key_to_be_missing() {
    assert_eq!(
        shape_of("['tax_id' => 'required_if:kind,business|string']"),
        "array{tax_id?: string}"
    );
}

#[test]
fn a_field_that_is_neither_required_nor_nullable_is_optional() {
    assert_eq!(shape_of("['active' => 'boolean']"), "array{active?: bool}");
}

#[test]
fn sometimes_makes_even_a_required_field_optional() {
    assert_eq!(
        shape_of("['nickname' => 'sometimes|required|string']"),
        "array{nickname?: string}"
    );
}

#[test]
fn array_form_rules_are_read_like_pipe_form() {
    assert_eq!(
        shape_of("['age' => ['required', 'integer']]"),
        "array{age: int}"
    );
}

#[test]
fn rule_parameters_do_not_confuse_the_type() {
    assert_eq!(
        shape_of("['published_at' => 'required|date_format:Y-m-d']"),
        "array{published_at: string}"
    );
}

#[test]
fn a_rule_object_keeps_the_key_but_loses_the_type() {
    // `Rule::unique(…)` still validated something; we cannot say what, so the
    // key survives as `mixed` rather than dropping out of the shape.
    assert_eq!(
        shape_of("['email' => [Rule::unique('users')]]"),
        "array{email?: mixed}"
    );
}

#[test]
fn a_required_rule_object_is_still_a_required_key() {
    assert_eq!(
        shape_of("['email' => ['required', Rule::unique('users')]]"),
        "array{email: mixed}"
    );
}

#[test]
fn unknown_constraint_rules_leave_the_type_open() {
    assert_eq!(
        shape_of("['token' => 'required|confirmed']"),
        "array{token: mixed}"
    );
}

#[test]
fn file_rules_resolve_to_uploaded_file() {
    assert_eq!(
        shape_of("['avatar' => 'required|image|max:2048']"),
        "array{avatar: \\Illuminate\\Http\\UploadedFile}"
    );
}

#[test]
fn numeric_admits_both_number_types() {
    assert_eq!(
        shape_of("['price' => 'required|numeric']"),
        "array{price: int|float}"
    );
}

// ─── Enum rules ─────────────────────────────────────────────────────────────

#[test]
fn a_string_backed_enum_rule_types_the_field_as_a_string() {
    // The validated array holds the raw input, not the enum case.
    assert_eq!(
        shape_of("['role' => ['required', new Enum(Role::class)]]"),
        "array{role: string}"
    );
}

#[test]
fn an_int_backed_enum_rule_types_the_field_as_an_int() {
    assert_eq!(
        shape_of("['level' => ['required', new Enum(Level::class)]]"),
        "array{level: int}"
    );
}

#[test]
fn the_rule_enum_shorthand_reads_like_the_rule_object() {
    assert_eq!(
        shape_of("['role' => ['required', Rule::enum(Role::class)]]"),
        "array{role: string}"
    );
}

#[test]
fn a_fluent_enum_rule_still_names_its_enum() {
    assert_eq!(
        shape_of("['role' => ['required', Rule::enum(Role::class)->only([Role::Admin])]]"),
        "array{role: string}"
    );
}

#[test]
fn an_enum_rule_written_without_an_array_is_read_the_same_way() {
    assert_eq!(
        shape_of("['role' => new Enum(Role::class)]"),
        "array{role?: string}"
    );
}

#[test]
fn a_nullable_enum_field_keeps_its_null() {
    assert_eq!(
        shape_of("['role' => ['nullable', new Enum(Role::class)]]"),
        "array{role?: ?string}"
    );
}

#[test]
fn a_pure_enum_rule_stays_mixed() {
    // A non-backed enum has no raw scalar form, so nothing can be claimed
    // about the validated value.
    assert_eq!(
        shape_of("['suit' => ['required', new Enum(Suit::class)]]"),
        "array{suit: mixed}"
    );
}

#[test]
fn an_unresolvable_enum_class_stays_mixed() {
    assert_eq!(
        shape_of("['role' => ['required', new Enum(Unknown::class)]]"),
        "array{role: mixed}"
    );
}

#[test]
fn a_declared_type_rule_still_wins_over_the_enum_backing_type() {
    assert_eq!(
        shape_of("['level' => ['required', 'string', new Enum(Level::class)]]"),
        "array{level: string}"
    );
}

#[test]
fn an_enum_rule_resolves_its_class_through_the_declaring_files_imports() {
    let source = "<?php\nnamespace App\\Http\\Requests;\nuse App\\Enums\\Role;\nuse Illuminate\\Validation\\Rules\\Enum;\n";
    let rules = rules_from_array_text_in_context(
        "['role' => [new Enum(Role::class)]]",
        source,
        source.len() as u32,
    )
    .unwrap();
    assert_eq!(
        rules.entries[0].enum_class.as_deref(),
        Some("App\\Enums\\Role")
    );
}

#[test]
fn an_unimported_enum_class_resolves_against_the_declaring_namespace() {
    let source = "<?php\nnamespace App\\Enums;\nuse Illuminate\\Validation\\Rule;\n";
    let rules = rules_from_array_text_in_context(
        "['role' => [Rule::enum(Role::class)]]",
        source,
        source.len() as u32,
    )
    .unwrap();
    assert_eq!(
        rules.entries[0].enum_class.as_deref(),
        Some("App\\Enums\\Role")
    );
}

// ─── Nesting ────────────────────────────────────────────────────────────────

#[test]
fn wildcard_children_build_a_list_of_shapes() {
    assert_eq!(
        shape_of(
            "[
                'items' => 'required|array',
                'items.*.id' => 'required|integer',
                'items.*.note' => 'nullable|string',
            ]"
        ),
        "array{items: list<array{id: int, note?: ?string}>}"
    );
}

#[test]
fn a_bare_wildcard_lists_its_scalar() {
    assert_eq!(
        shape_of(
            "[
                'tags' => 'required|array',
                'tags.*' => 'string',
            ]"
        ),
        "array{tags: list<string>}"
    );
}

#[test]
fn a_nullable_parent_keeps_its_null_alongside_the_child_shape() {
    // The child rules describe what a present `items` holds; `nullable` on
    // `items` itself still says the value may be null.
    assert_eq!(
        shape_of(
            "[
                'items' => 'required|nullable|array',
                'items.*.id' => 'required|integer',
            ]"
        ),
        "array{items: ?list<array{id: int}>}"
    );
}

#[test]
fn dotted_keys_build_a_nested_shape() {
    assert_eq!(
        shape_of(
            "[
                'owner' => 'required|array',
                'owner.email' => 'required|email',
            ]"
        ),
        "array{owner: array{email: string}}"
    );
}

#[test]
fn an_undeclared_parent_is_required_when_a_child_is() {
    // No `'owner' => …` entry, but `owner.email` is required, so the parent
    // key cannot be absent.
    assert_eq!(
        shape_of("['owner.email' => 'required|email']"),
        "array{owner: array{email: string}}"
    );
}

#[test]
fn an_undeclared_parent_is_optional_when_every_child_is() {
    assert_eq!(
        shape_of("['owner.email' => 'email']"),
        "array{owner?: array{email?: string}}"
    );
}

// ─── Bailing out ────────────────────────────────────────────────────────────

#[test]
fn a_computed_key_abandons_the_shape() {
    // The key set is incomplete, so a shape would report real input as an
    // unknown key.
    let rules = rules_from_array_text("['name' => 'required', $dynamic => 'required']").unwrap();
    assert!(!rules.keys_complete);
    assert!(rules_to_shape(&rules, &test_enum).is_none());
}

#[test]
fn a_spread_abandons_the_shape() {
    let rules = rules_from_array_text("[...$base, 'name' => 'required']").unwrap();
    assert!(rules_to_shape(&rules, &test_enum).is_none());
}

#[test]
fn an_empty_rules_array_has_no_shape() {
    assert!(rules_from_array_text("[]").is_none());
}

// ─── Member lookup and narrowing ────────────────────────────────────────────

fn rules(array_text: &str) -> RulesArray {
    rules_from_array_text(array_text).expect("rules should parse")
}

#[test]
fn member_type_reads_a_single_key() {
    let rules = rules("['name' => 'required|string', 'age' => 'nullable|integer']");
    assert_eq!(
        rules_member_type(&rules, "age", &test_enum).map(|t| t.to_string()),
        Some("?int".to_string())
    );
}

#[test]
fn member_type_walks_dot_notation() {
    let rules = rules("['owner.email' => 'required|email']");
    assert_eq!(
        rules_member_type(&rules, "owner.email", &test_enum).map(|t| t.to_string()),
        Some("string".to_string())
    );
}

#[test]
fn member_type_of_an_unknown_key_is_none() {
    let rules = rules("['name' => 'required|string']");
    assert!(rules_member_type(&rules, "nope", &test_enum).is_none());
}

#[test]
fn only_keeps_the_listed_keys() {
    let shape = rules_to_shape(&rules(
        "['name' => 'required|string', 'age' => 'required|integer', 'city' => 'required|string']",
    ), &test_enum)
    .unwrap();
    let narrowed = narrow_shape(&shape, &["name".to_string(), "city".to_string()], true).unwrap();
    assert_eq!(narrowed.to_string(), "array{name: string, city: string}");
}

#[test]
fn except_drops_the_listed_keys() {
    let shape = rules_to_shape(
        &rules("['name' => 'required|string', 'age' => 'required|integer']"),
        &test_enum,
    )
    .unwrap();
    let narrowed = narrow_shape(&shape, &["age".to_string()], false).unwrap();
    assert_eq!(narrowed.to_string(), "array{name: string}");
}

#[test]
fn a_key_list_reads_both_the_array_and_the_variadic_form() {
    assert_eq!(key_list(&["['name', 'city']"]).unwrap(), ["name", "city"]);
    assert_eq!(key_list(&["'name'", "'city'"]).unwrap(), ["name", "city"]);
}

#[test]
fn a_literal_empty_key_list_is_an_empty_list() {
    // `only([])` really does narrow to nothing, so this is not the same as a
    // list that could not be read.
    assert_eq!(key_list(&["[]"]).unwrap(), Vec::<String>::new());
}

#[test]
fn an_unreadable_key_makes_the_whole_list_none() {
    // A partial list narrows further than the call does, which turns reading a
    // dropped key into a bogus unknown-key error.
    assert!(key_list(&["$keys"]).is_none());
    assert!(key_list(&["['title', $extra]"]).is_none());
    assert!(key_list(&["'title'", "$extra"]).is_none());
}

#[test]
fn nullable_numeric_renders_as_a_nullable_union() {
    // Documented in `examples/laravel/app/Demo.php::validatedArrayShape`.
    assert_eq!(
        shape_of("['dough_temp' => 'nullable|numeric']"),
        "array{dough_temp?: ?int|float}"
    );
}

#[test]
fn the_bakery_demo_rules_produce_the_documented_shape() {
    // Mirrors App\Http\Requests\StoreBakeryRequest::rules() so the comments
    // in the Laravel demo cannot drift from what the engine infers.
    assert_eq!(
        shape_of(
            "[
                'name' => 'required|string|max:255',
                'apricot' => 'boolean',
                'dough_temp' => 'nullable|numeric',
                'notes' => 'array',
                'notes.*.body' => 'required|string',
                'owner.email' => 'required|email',
                'flavor' => ['required', new Enum(JamFlavor::class)],
                'batch_size' => ['required', Rule::enum(BatchSize::class)],
            ]"
        ),
        "array{name: string, apricot?: bool, dough_temp?: ?int|float, \
notes?: list<array{body: string}>, owner: array{email: string}, \
flavor: string, batch_size: int}"
    );
}
