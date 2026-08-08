use std::sync::Arc;

use super::{
    enrich_builder_type_in_scope, merge_keyed_type, merge_push_type, normalize_array_key_type,
};
use crate::atom::atom;
use crate::php_type::PhpType;
use crate::test_fixtures::make_class;

use crate::type_engine::resolver::Loaders;
use crate::types::{ClassInfo, ResolvedType};

fn make_model(name: &str) -> ClassInfo {
    let mut class = make_class(name);
    class.parent_class = Some(atom("Illuminate\\Database\\Eloquent\\Model"));
    class
}

fn model_loader(name: &str) -> Option<Arc<ClassInfo>> {
    if name == "Illuminate\\Database\\Eloquent\\Model" {
        Some(Arc::new(make_class(
            "Illuminate\\Database\\Eloquent\\Model",
        )))
    } else if name == "App\\Models\\User" {
        Some(Arc::new(make_model("App\\Models\\User")))
    } else {
        None
    }
}

#[test]
fn enrich_scope_method_with_builder_type() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "scopeActive",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(result, Some(PhpType::parse("Builder<App\\Models\\User>")));
}

#[test]
fn enrich_scope_method_with_fqn_builder() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Illuminate\\Database\\Eloquent\\Builder"),
        "scopeActive",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(
        result,
        Some(PhpType::parse(
            "Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>"
        ))
    );
}

#[test]
fn enrich_skips_non_scope_method() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "getName",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(result, None);
}

#[test]
fn enrich_skips_bare_scope_name() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "scope",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(result, None);
}

#[test]
fn enrich_skips_non_model_class() {
    let plain = make_class("App\\Services\\SomeService");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "scopeActive",
        false,
        &plain,
        &model_loader,
    );
    assert_eq!(result, None);
}

#[test]
fn enrich_skips_non_builder_type() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Collection"),
        "scopeActive",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(result, None);
}

#[test]
fn enrich_skips_builder_with_existing_generics() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder<User>"),
        "scopeActive",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(result, None);
}

#[test]
fn enrich_scope_multi_word_method_name() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "scopeByAuthor",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(result, Some(PhpType::parse("Builder<App\\Models\\User>")));
}

#[test]
fn enrich_scope_with_fqn_builder() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Illuminate\\Database\\Eloquent\\Builder"),
        "scopeActive",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(
        result,
        Some(PhpType::parse(
            "Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>"
        ))
    );
}

// ── #[Scope] attribute tests ────────────────────────────────────────

#[test]
fn enrich_scope_attribute_method_with_builder_type() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "active",
        true,
        &model,
        &model_loader,
    );
    assert_eq!(result, Some(PhpType::parse("Builder<App\\Models\\User>")));
}

#[test]
fn enrich_scope_attribute_with_fqn_builder() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Illuminate\\Database\\Eloquent\\Builder"),
        "active",
        true,
        &model,
        &model_loader,
    );
    assert_eq!(
        result,
        Some(PhpType::parse(
            "Illuminate\\Database\\Eloquent\\Builder<App\\Models\\User>"
        ))
    );
}

#[test]
fn enrich_scope_attribute_skips_non_model_class() {
    let plain = make_class("App\\Services\\SomeService");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "active",
        true,
        &plain,
        &model_loader,
    );
    assert_eq!(result, None);
}

#[test]
fn enrich_scope_attribute_skips_non_builder_type() {
    let model = make_model("App\\Models\\User");
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Collection"),
        "active",
        true,
        &model,
        &model_loader,
    );
    assert_eq!(result, None);
}

#[test]
fn enrich_no_scope_attribute_and_no_convention_skips() {
    let model = make_model("App\\Models\\User");
    // Not a scopeX name and no attribute → should skip.
    let result = enrich_builder_type_in_scope(
        &PhpType::parse("Builder"),
        "active",
        false,
        &model,
        &model_loader,
    );
    assert_eq!(result, None);
}

// ── Variable resolution: static chain assignment ────────────────────

/// `$result = Foo::create()->process(); $result->` should resolve
/// through the static call chain when `resolve_variable_types` is
/// called directly.
#[test]
fn resolve_var_from_static_method_chain_assignment() {
    use crate::types::MethodInfo;

    let content = r#"<?php
class Processor {
    public function getOutput(): string { return ''; }
}

class Builder {
    public function process(): Processor { return new Processor(); }
}

class Factory {
    public static function create(): Builder { return new Builder(); }
}

function test() {
    $result = Factory::create()->process();
    $result->
}
"#;
    // Classes that exist in this file
    let processor = {
        let mut c = make_class("Processor");
        c.methods.push(Arc::new(MethodInfo {
            is_static: false,
            ..MethodInfo::virtual_method("getOutput", Some("string"))
        }));
        c
    };
    let builder = {
        let mut c = make_class("Builder");
        c.methods.push(Arc::new(MethodInfo {
            is_static: false,
            ..MethodInfo::virtual_method("process", Some("Processor"))
        }));
        c
    };
    let factory = {
        let mut c = make_class("Factory");
        c.methods.push(Arc::new(MethodInfo {
            is_static: true,
            ..MethodInfo::virtual_method("create", Some("Builder"))
        }));
        c
    };

    let all_classes: Vec<Arc<ClassInfo>> = vec![
        Arc::new(processor.clone()),
        Arc::new(builder.clone()),
        Arc::new(factory.clone()),
    ];
    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Processor" => Some(Arc::new(processor.clone())),
            "Builder" => Some(Arc::new(builder.clone())),
            "Factory" => Some(Arc::new(factory.clone())),
            _ => None,
        }
    };

    // cursor_offset: find the position of `$result->` on the last
    // meaningful line.  We need an offset inside `function test()`.
    let cursor_offset = content.find("$result->").unwrap() as u32 + 9; // after `->`

    let results = ResolvedType::into_classes(super::resolve_variable_types(
        "$result",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    ));

    let names: Vec<&str> = results.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"Processor"),
        "$result should resolve to Processor via Factory::create()->process(), got: {:?}",
        names
    );
}

/// Cross-file scenario: `$user = User::factory()->create(); $user->`
/// where `factory()` comes from a trait with `@return TFactory` and
/// `create()` comes from the Factory base class with `@return TModel`.
///
/// This mirrors the Laravel `HasFactory` + `Factory` pattern that the
/// integration test `test_factory_variable_assignment_then_create`
/// exercises through the full LSP handler.
#[test]
fn resolve_var_from_cross_file_factory_chain() {
    use crate::types::MethodInfo;

    // The PHP source that the variable resolver will parse.
    // Classes are NOT defined here — they come from class_loader.
    let content = r#"<?php
use App\Models\User;
function test() {
    $user = User::factory()->create();
    $user->
}
"#;

    // ── Build the class graph ───────────────────────────────────

    // HasFactory trait: `public static function factory(): TFactory`
    // After trait merging with convention-based subs, User gets
    // `factory()` with return type `Database\Factories\UserFactory`.
    let has_factory_trait = {
        let mut c = make_class("HasFactory");
        c.file_namespace = Some(atom("Illuminate\\Database\\Eloquent\\Factories"));
        c.template_params = vec![atom("TFactory")];
        c.methods.push(Arc::new(MethodInfo {
            is_static: true,
            ..MethodInfo::virtual_method("factory", Some("TFactory"))
        }));
        c
    };

    // Factory base class: `public function create(): TModel`
    let factory_base = {
        let mut c = make_class("Factory");
        c.file_namespace = Some(atom("Illuminate\\Database\\Eloquent\\Factories"));
        c.template_params = vec![atom("TModel")];
        c.methods.push(Arc::new(MethodInfo::virtual_method(
            "create",
            Some("TModel"),
        )));
        c.methods
            .push(Arc::new(MethodInfo::virtual_method("make", Some("TModel"))));
        c
    };

    // UserFactory extends Factory — convention says TModel = User.
    let user_factory = {
        let mut c = make_class("UserFactory");
        c.file_namespace = Some(atom("Database\\Factories"));
        c.parent_class = Some(atom("Illuminate\\Database\\Eloquent\\Factories\\Factory"));
        // The virtual member provider would synthesize create()/make()
        // returning User, but for this unit test we add them directly
        // with the substituted return type.
        c.methods.push(Arc::new(MethodInfo::virtual_method(
            "create",
            Some("App\\Models\\User"),
        )));
        c.methods.push(Arc::new(MethodInfo::virtual_method(
            "make",
            Some("App\\Models\\User"),
        )));
        c
    };

    // Model base class
    let model_base = make_class("Model");

    // User extends Model, uses HasFactory.
    // After trait merging, factory() returns UserFactory.
    let user = {
        let mut c = make_class("User");
        c.file_namespace = Some(atom("App\\Models"));
        c.parent_class = Some(atom("Illuminate\\Database\\Eloquent\\Model"));
        c.used_traits = vec![atom(
            "Illuminate\\Database\\Eloquent\\Factories\\HasFactory",
        )];
        // Simulate the result of trait merging with convention-based
        // TFactory substitution: factory() returns UserFactory FQN.
        c.methods.push(Arc::new(MethodInfo {
            is_static: true,
            ..MethodInfo::virtual_method("factory", Some("Database\\Factories\\UserFactory"))
        }));
        c.methods.push(Arc::new(MethodInfo::virtual_method(
            "greet",
            Some("string"),
        )));
        c
    };

    let all_classes: Vec<Arc<ClassInfo>> = vec![];

    let user_c = user.clone();
    let user_factory_c = user_factory.clone();
    let factory_base_c = factory_base.clone();
    let model_base_c = model_base.clone();
    let has_factory_c = has_factory_trait.clone();
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "User" | "App\\Models\\User" => Some(Arc::new(user_c.clone())),
            "UserFactory" | "Database\\Factories\\UserFactory" => {
                Some(Arc::new(user_factory_c.clone()))
            }
            "Factory" | "Illuminate\\Database\\Eloquent\\Factories\\Factory" => {
                Some(Arc::new(factory_base_c.clone()))
            }
            "Model" | "Illuminate\\Database\\Eloquent\\Model" => {
                Some(Arc::new(model_base_c.clone()))
            }
            "HasFactory" | "Illuminate\\Database\\Eloquent\\Factories\\HasFactory" => {
                Some(Arc::new(has_factory_c.clone()))
            }
            _ => None,
        }
    };

    let cursor_offset = content.find("$user->").unwrap() as u32 + 7;

    let results = ResolvedType::into_classes(super::resolve_variable_types(
        "$user",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    ));

    let names: Vec<&str> = results.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"User"),
        "$user should resolve to User via User::factory()->create(), got: {:?}",
        names
    );
}

#[test]
fn scalar_literal_reproduction_preserves_ternary_literals() {
    let content = r#"<?php
function test(bool $flag): void {
    $choice = $flag ? 'asc' : 'desc';
    echo $choice;
}
"#;
    let cursor_offset = content.find("echo $choice").unwrap() as u32 + 5;

    let results = super::resolve_variable_types(
        "$choice",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert_eq!(
        ResolvedType::types_joined(&results).to_string(),
        "'asc'|'desc'"
    );
}

fn resolve_literal_test_var(content: &str, var_name: &str) -> String {
    let cursor_offset = content.rfind(var_name).unwrap() as u32;
    let results = super::resolve_variable_types(
        var_name,
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );
    ResolvedType::types_joined(&results).to_string()
}

#[test]
fn resolve_var_preserves_scalar_literal_assignments() {
    let content = r#"<?php
function test() {
    $string = 'draft';
    $integer = 42;
    $hex = 0x2A;
    $float = 1.25;
    $numeric_string = '123';
    $negative = -7;
    $positive = +8;

    echo $string, $integer, $hex, $float, $numeric_string, $negative, $positive;
}
"#;

    let actual = [
        resolve_literal_test_var(content, "$string"),
        resolve_literal_test_var(content, "$integer"),
        resolve_literal_test_var(content, "$hex"),
        resolve_literal_test_var(content, "$float"),
        resolve_literal_test_var(content, "$numeric_string"),
        resolve_literal_test_var(content, "$negative"),
        resolve_literal_test_var(content, "$positive"),
    ];

    assert_eq!(actual, ["'draft'", "42", "42", "1.25", "'123'", "-7", "8"]);
}

#[test]
fn resolve_var_preserves_literals_through_compound_expressions() {
    let content = r#"<?php
function test(
    bool $flag,
    int $broad_int,
    float $broad_float,
    string $broad_string,
    ?string $maybe_string,
) {
    $ternary = $flag ? 'asc' : 'desc';
    $matched = match ($flag) { true => 1, false => 2 };
    $parenthesized = ('wrapped');
    $operand = 7;
    $negative_parenthesized = -(7);
    $positive_variable = +$operand;
    $negative_union = -($flag ? 1 : 2);
    $double_negative = -(-7);
    $negative_broad = -$broad_int;
    $positive_broad = +$broad_int;
    $broad_coalesced = $broad_string ?? 'fallback';
    $nullable_coalesced = $maybe_string ?? 'fallback';
    $null_coalesced = null ?? 'fallback';
    $parenthesized_null_coalesced = (null) ?? 'fallback';
    $mixed_numeric = $flag ? $broad_float : 1;
    $runtime_domains = match ($broad_int) {
        0 => 1,
        1 => $broad_int,
        default => $broad_float,
    };

    echo $ternary, $matched, $parenthesized, $negative_parenthesized,
        $positive_variable, $negative_union, $double_negative, $negative_broad,
        $positive_broad, $broad_coalesced, $nullable_coalesced, $null_coalesced,
        $parenthesized_null_coalesced, $mixed_numeric, $runtime_domains;
}
"#;

    assert_eq!(
        resolve_literal_test_var(content, "$ternary"),
        "'asc'|'desc'"
    );
    assert_eq!(resolve_literal_test_var(content, "$matched"), "1|2");
    assert_eq!(
        resolve_literal_test_var(content, "$parenthesized"),
        "'wrapped'"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$negative_parenthesized"),
        "-7"
    );
    assert_eq!(resolve_literal_test_var(content, "$positive_variable"), "7");
    assert_eq!(
        resolve_literal_test_var(content, "$negative_union"),
        "-1|-2"
    );
    assert_eq!(resolve_literal_test_var(content, "$double_negative"), "7");
    assert_eq!(resolve_literal_test_var(content, "$negative_broad"), "int");
    assert_eq!(resolve_literal_test_var(content, "$positive_broad"), "int");
    assert_eq!(
        resolve_literal_test_var(content, "$broad_coalesced"),
        "string"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$nullable_coalesced"),
        "string"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$null_coalesced"),
        "'fallback'"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$parenthesized_null_coalesced"),
        "'fallback'"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$mixed_numeric"),
        "float|1"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$runtime_domains"),
        "int|float"
    );
}

#[test]
fn collection_tracking_widens_only_at_mutable_collection_boundaries() {
    let content = r#"<?php
/**
 * @param Iterator<int, 'draft'> $iterator
 * @param Iterator<int, 'left'>|Iterator<string, 'right'> $union_iterator
 */
function test(bool $flag, string $key, $iterator, $union_iterator) {
    $shape = [
        'direction' => $flag ? 'asc' : 'desc',
        'code' => match ($flag) { true => 1, false => 2 },
    ];
    $list = [$flag ? 'left' : 'right', $flag ? 1 : 2];
    $nested = ['meta' => [$flag ? 'a' : 'b']];

    $pushed = [];
    $pushed[] = $flag ? 'left' : 'right';

    $written = [];
    $written['state'] = match ($flag) {
        true => 'draft',
        false => 'published',
    };

    $dynamic = [];
    $dynamic[$key] = $flag ? 1 : 2;

    $state = 'draft';
    $from_variable = [$state];
    $spread_source = [$state];
    $spread = [...$spread_source];
    $tuple_container = [['left', 'right']];
    $tuple = $tuple_container[0];
    $tuple_spread = [...$tuple];
    $mapped = array_map(fn($item) => 'mapped', [1]);
    $converted = iterator_to_array($iterator);
    $converted_union = iterator_to_array($union_iterator);
    $renumbered = iterator_to_array($union_iterator, false);

    /** @var Box<'draft'> $box */
    $structural = [$box];

    echo $shape, $list, $nested, $pushed, $written, $dynamic,
        $from_variable, $spread, $tuple_spread, $mapped, $converted,
        $converted_union, $renumbered, $structural;
}
"#;

    assert_eq!(
        resolve_literal_test_var(content, "$shape"),
        "array{direction: string, code: int}"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$list"),
        "list<string|int>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$nested"),
        "array{meta: array{string}}"
    );
    assert_eq!(resolve_literal_test_var(content, "$pushed"), "list<string>");
    assert_eq!(
        resolve_literal_test_var(content, "$written"),
        "array{state: string}"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$dynamic"),
        "array<int|string, int>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$from_variable"),
        "list<string>"
    );
    assert_eq!(resolve_literal_test_var(content, "$spread"), "list<string>");
    assert_eq!(
        resolve_literal_test_var(content, "$tuple_spread"),
        "list<string>"
    );
    assert_eq!(resolve_literal_test_var(content, "$mapped"), "list<string>");
    assert_eq!(
        resolve_literal_test_var(content, "$converted"),
        "array<int, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$converted_union"),
        "array<int|string, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$renumbered"),
        "list<string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$structural"),
        "list<Box<'draft'>>"
    );
}

/// `array_map` falls back to the input element type when the callback's
/// return cannot be inferred. That guess contradicts the callback whenever
/// the elements are scalars, which is exactly what a converting callback
/// such as `'intval'` is there to change.
#[test]
fn array_map_does_not_claim_the_input_element_type_for_an_opaque_callback() {
    let content = r#"<?php
/** @param list<string> $ids */
function test(array $ids) {
    $converted = array_map('intval', $ids);

    echo $converted;
}
"#;

    assert_ne!(
        resolve_literal_test_var(content, "$converted"),
        "list<string>"
    );
}

#[test]
fn control_flow_merges_absorb_literals_redundant_with_broad_scalar_branches() {
    let content = r#"<?php
function test(bool $flag, string $broad) {
    if ($flag) {
        $result = 'fixed';
    } else {
        $result = $broad;
    }

    echo $result;
}
"#;

    assert_eq!(resolve_literal_test_var(content, "$result"), "string");
}

#[test]
fn literal_mutations_and_numeric_consumers_invalidate_only_changed_values() {
    let content = r#"<?php
function test(bool $flag) {
    $numeric = '123';
    $numeric++;
    $alphabetic = 'abc';
    $alphabetic++;
    $unchanged = 'abc';
    $unchanged--;
    $integer = 1;
    $integer++;
    $float = 1.5;
    $float++;
    $number_union = $flag ? 1 : 1.5;
    $number_union++;

    $text = 'abc';
    $text[0] = 'z';
    $string_object = (object) 'x';
    $int_object = (object) 1;
    $union_object = (object) ($flag ? 1 : 'x');

    $integer_sum = 1 + 2;
    $float_sum = 1 + 2.5;
    $division = 1 / 2;

    echo $numeric, $alphabetic, $unchanged, $integer, $float, $number_union, $text,
        $string_object, $int_object, $union_object, $integer_sum, $float_sum,
        $division;
}
"#;

    assert_eq!(resolve_literal_test_var(content, "$numeric"), "int|float");
    assert_eq!(resolve_literal_test_var(content, "$alphabetic"), "string");
    assert_eq!(resolve_literal_test_var(content, "$unchanged"), "'abc'");
    assert_eq!(resolve_literal_test_var(content, "$integer"), "int");
    assert_eq!(resolve_literal_test_var(content, "$float"), "float");
    assert_eq!(
        resolve_literal_test_var(content, "$number_union"),
        "int|float"
    );
    assert_eq!(resolve_literal_test_var(content, "$text"), "string");
    assert_eq!(
        resolve_literal_test_var(content, "$string_object"),
        "object{scalar: string}"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$int_object"),
        "object{scalar: int}"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$union_object"),
        "object{scalar: int|string}"
    );
    assert_eq!(resolve_literal_test_var(content, "$integer_sum"), "int");
    assert_eq!(resolve_literal_test_var(content, "$float_sum"), "float");
    assert_eq!(resolve_literal_test_var(content, "$division"), "int|float");
}

#[test]
fn collection_key_boundaries_normalize_literal_and_coercible_keys() {
    let content = r#"<?php
function test(bool $flag, ?int $nullable_key, string $broad_string_key) {
    $int_key = 1;
    $int_map = [];
    $int_map[$int_key] = 'x';

    $float_key = 1.5;
    $float_map = [];
    $float_map[$float_key] = 'x';

    $union_key = $flag ? 1 : 'id';
    $union_map = [];
    $union_map[$union_key] = 'x';

    $null_key = null;
    $null_map = [];
    $null_map[$null_key] = 'x';

    $nullable_map = [];
    $nullable_map[$nullable_key] = 'x';

    $decimal_string_key = '8';
    $decimal_string_map = [];
    $decimal_string_map[$decimal_string_key] = 'x';

    $leading_zero_key = '08';
    $leading_zero_map = [];
    $leading_zero_map[$leading_zero_key] = 'x';

    $broad_string_map = [];
    $broad_string_map[$broad_string_key] = 'x';

    $direct_decimal_map = [];
    $direct_decimal_map['8'] = 'x';
    $direct_negative_map = [];
    $direct_negative_map['-2'] = 'x';
    $direct_leading_zero_map = [];
    $direct_leading_zero_map['08'] = 'x';
    $direct_plus_map = [];
    $direct_plus_map['+8'] = 'x';
    $direct_decimal_float_map = [];
    $direct_decimal_float_map['1.5'] = 'x';

    echo $int_map, $float_map, $union_map, $null_map, $nullable_map,
        $decimal_string_map, $leading_zero_map, $broad_string_map,
        $direct_decimal_map, $direct_negative_map, $direct_leading_zero_map,
        $direct_plus_map, $direct_decimal_float_map;
}
"#;

    assert_eq!(
        resolve_literal_test_var(content, "$int_map"),
        "array<int, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$float_map"),
        "array<int, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$union_map"),
        "array<int|string, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$null_map"),
        "array<string, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$nullable_map"),
        "array<int|string, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$decimal_string_map"),
        "array<int, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$leading_zero_map"),
        "array<string, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$broad_string_map"),
        "array<int|string, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$direct_decimal_map"),
        "array<int, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$direct_negative_map"),
        "array<int, string>"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$direct_leading_zero_map"),
        "array{08: string}"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$direct_plus_map"),
        "array{+8: string}"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$direct_decimal_float_map"),
        "array{'1.5': string}"
    );
}

#[test]
fn collection_key_normalization_preserves_non_numeric_string_domains() {
    assert_eq!(
        normalize_array_key_type(&PhpType::parse("class-string<Foo>")),
        Some(PhpType::string())
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::parse("interface-string<Foo>")),
        Some(PhpType::string())
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::parse("class-string")),
        Some(PhpType::string())
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::string())
            .unwrap()
            .to_string(),
        "int|string"
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::named(atom("number"))),
        Some(PhpType::int())
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::named(atom("Number"))),
        None
    );
    assert_eq!(
        normalize_array_key_type(&PhpType::literal_string_raw("\"\\x38\""))
            .unwrap()
            .to_string(),
        "int|string"
    );
}

#[test]
fn mutable_collection_merges_normalize_complete_existing_domains() {
    assert_eq!(
        merge_push_type(
            &PhpType::parse("list<'existing'>"),
            &PhpType::literal_string_raw("'new'"),
        ),
        PhpType::parse("list<string>")
    );
    assert_eq!(
        merge_keyed_type(
            &PhpType::parse("array<'existing-key', 'existing-value'>"),
            &PhpType::literal_string_raw("'new-key'"),
            &PhpType::literal_string_raw("'new-value'"),
        ),
        PhpType::parse("array<string, string>")
    );
    assert_eq!(
        merge_keyed_type(
            &PhpType::parse("list<'existing-value'>"),
            &PhpType::literal_string_raw("'named-key'"),
            &PhpType::literal_string_raw("'new-value'"),
        ),
        PhpType::parse("array<int|string, string>")
    );
}

#[test]
fn nullable_increment_and_decrement_keep_operator_specific_domains() {
    let content = r#"<?php
function test(
    ?int $inc_int,
    ?int $dec_int,
    ?float $inc_float,
    ?float $dec_float,
    ?string $inc_string,
    ?string $dec_string,
) {
    $inc_int++;
    $dec_int--;
    ++$inc_float;
    --$dec_float;
    $inc_string++;
    $dec_string--;

    echo $inc_int, $dec_int, $inc_float, $dec_float, $inc_string, $dec_string;
}
"#;

    assert_eq!(resolve_literal_test_var(content, "$inc_int"), "int");
    assert_eq!(resolve_literal_test_var(content, "$dec_int"), "?int");
    assert_eq!(resolve_literal_test_var(content, "$inc_float"), "int|float");
    assert_eq!(resolve_literal_test_var(content, "$dec_float"), "?float");
    assert_eq!(
        resolve_literal_test_var(content, "$inc_string"),
        "int|float|string"
    );
    assert_eq!(
        resolve_literal_test_var(content, "$dec_string"),
        "int|float|string|null"
    );
}

// ── Shape tracking: incremental key assignments ─────────────────────

/// `$data = []; $data['name'] = 'John'; $data['age'] = 42;`
/// The unified pipeline should produce `array{name: string, age: int}`.
#[test]
fn resolve_var_shape_from_incremental_key_assignments() {
    let content = r#"<?php
function test() {
    $data = [];
    $data['name'] = 'John';
    $data['age'] = 42;
    $data['x']
}
"#;
    let cursor_offset = content.find("$data['x']").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$data",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $data to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("name: string"),
        "Shape should contain 'name: string', got: {ts}"
    );
    assert!(
        ts.contains("age: int"),
        "Shape should contain 'age: int', got: {ts}"
    );
}

/// A base assignment followed by incremental keys should merge the
/// shape keys into the base type.
#[test]
fn resolve_var_shape_merges_with_base_assignment() {
    let content = r#"<?php
function test() {
    $config = ['host' => 'localhost'];
    $config['port'] = 3306;
    $config['x']
}
"#;
    let cursor_offset = content.find("$config['x']").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$config",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $config to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    // The base array{host: string} should be merged with the new key.
    assert!(
        ts.contains("port: int"),
        "Shape should contain 'port: int', got: {ts}"
    );
}

/// Overwriting an existing shape key should update its type.
#[test]
fn resolve_var_shape_key_override() {
    let content = r#"<?php
function test() {
    $data = [];
    $data['value'] = 'hello';
    $data['value'] = 42;
    $data['x']
}
"#;
    let cursor_offset = content.find("$data['x']").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$data",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $data to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("value: int"),
        "Shape key 'value' should be overridden to int, got: {ts}"
    );
    assert!(
        !ts.contains("value: string"),
        "Old type 'string' should be gone, got: {ts}"
    );
}

// ── List tracking: push assignments ─────────────────────────────────

/// `$items = []; $items[] = new User();`
/// The unified pipeline should produce `list<User>`.
#[test]
fn resolve_var_list_from_push_assignments() {
    let content = r#"<?php
class User { public string $name; }
function test() {
    $items = [];
    $items[] = new User();
    $items[0]->
}
"#;
    let user = make_class("User");
    let all_classes: Vec<Arc<ClassInfo>> = vec![Arc::new(user.clone())];
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "User" {
            Some(Arc::new(make_class("User")))
        } else {
            None
        }
    };

    let cursor_offset = content.find("$items[0]->").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$items",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $items to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("User"),
        "List element type should contain User, got: {ts}"
    );
    assert!(
        ts.starts_with("list<"),
        "Should be a list<> type, got: {ts}"
    );
}

/// Multiple push assignments with different types should union.
#[test]
fn resolve_var_list_from_push_union() {
    let content = r#"<?php
function test() {
    $items = [];
    $items[] = 'hello';
    $items[] = 42;
    $items[0]
}
"#;
    let cursor_offset = content.find("$items[0]").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$items",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $items to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("string") && ts.contains("int"),
        "List should contain string|int union, got: {ts}"
    );
}

/// Push of the same type should not duplicate.
#[test]
fn resolve_var_list_push_deduplicates() {
    let content = r#"<?php
function test() {
    $items = [];
    $items[] = 'a';
    $items[] = 'b';
    $items[0]
}
"#;
    let cursor_offset = content.find("$items[0]").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$items",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $items to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(
        ts, "list<string>",
        "Duplicate pushes of same type should not duplicate, got: {ts}"
    );
}

/// Reassignment resets push tracking: `$x = []; $x[] = 1; $x = []; $x[] = 'a';`
/// should produce `list<string>`, not `list<int|string>`.
#[test]
fn resolve_var_reassignment_resets_push_tracking() {
    let content = r#"<?php
function test() {
    $x = [];
    $x[] = 1;
    $x = [];
    $x[] = 'hello';
    $x[0]
}
"#;
    let cursor_offset = content.find("$x[0]").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$x",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $x to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(
        ts, "list<string>",
        "Reassignment should reset; only 'string' push should remain, got: {ts}"
    );
}

/// Numeric keys in `$var[0] = expr` should NOT be treated as shape entries.
#[test]
fn resolve_var_numeric_key_not_tracked_as_shape() {
    let content = r#"<?php
function test() {
    $data = [];
    $data[0] = 'hello';
    $data[1] = 42;
    echo $data;
}
"#;
    let cursor_offset = content.find("echo $data").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$data",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    // Numeric keys are not shape entries, so the type should stay as
    // the base `array` from `$data = []`.  The results may be empty
    // (just `array`) or contain `array` as a type string.
    let ts = if results.is_empty() {
        "array".to_string()
    } else {
        ResolvedType::types_joined(&results).to_string()
    };
    assert!(
        !ts.contains('{'),
        "Numeric keys should not produce a shape, got: {ts}"
    );
}

#[test]
fn resolve_var_from_parent_static_call() {
    use crate::types::MethodInfo;

    let content = r#"<?php
class Response {
    public function status(): int { return 200; }
    public function body(): string { return ''; }
}

class BaseConnector {
    protected function call(string $endpoint): Response
    {
        return new Response();
    }
}

class LoggedConnection extends BaseConnector {
    protected function call(string $endpoint): Response
    {
        $response = parent::call($endpoint);
        $response->
    }
}
"#;

    let response = {
        let mut c = make_class("Response");
        c.methods.push(Arc::new(MethodInfo {
            is_static: false,
            ..MethodInfo::virtual_method("status", Some("int"))
        }));
        c.methods.push(Arc::new(MethodInfo {
            is_static: false,
            ..MethodInfo::virtual_method("body", Some("string"))
        }));
        c
    };
    let base = {
        let mut c = make_class("BaseConnector");
        c.methods.push(Arc::new(MethodInfo {
            is_static: false,
            ..MethodInfo::virtual_method("call", Some("Response"))
        }));
        c
    };
    let logged = {
        let mut c = make_class("LoggedConnection");
        c.parent_class = Some(atom("BaseConnector"));
        c.methods.push(Arc::new(MethodInfo {
            is_static: false,
            ..MethodInfo::virtual_method("call", Some("Response"))
        }));
        c
    };

    let all_classes: Vec<Arc<ClassInfo>> = vec![
        Arc::new(response.clone()),
        Arc::new(base.clone()),
        Arc::new(logged.clone()),
    ];
    let class_loader = |name: &str| -> Option<Arc<ClassInfo>> {
        match name {
            "Response" => Some(Arc::new(response.clone())),
            "BaseConnector" => Some(Arc::new(base.clone())),
            "LoggedConnection" => Some(Arc::new(logged.clone())),
            _ => None,
        }
    };

    let cursor_offset = content.find("$response->").unwrap() as u32 + 11;

    let results = ResolvedType::into_classes(super::resolve_variable_types(
        "$response",
        &logged,
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    ));

    let names: Vec<&str> = results.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"Response"),
        "$response should resolve to Response via parent::call(), got: {:?}",
        names
    );
}

/// Nested array access assignments like `$b['a']['b'] = 'x'` should
/// produce a nested array shape `array{a: array{b: string}}`.
#[test]
fn resolve_var_shape_from_nested_key_assignments() {
    let content = r#"<?php
function test() {
    $b['a']['a'] = 'a';
    $b['x']
}
"#;
    let cursor_offset = content.find("$b['x']").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$b",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $b to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("a: array{a: string}"),
        "Shape should contain nested 'a: array{{a: string}}', got: {ts}"
    );
}

/// Deeply nested key assignments like `$c['a']['b']['c'] = 42` should
/// produce `array{a: array{b: array{c: int}}}`.
#[test]
fn resolve_var_shape_from_deeply_nested_key_assignments() {
    let content = r#"<?php
function test() {
    $config['db']['host']['primary'] = 'localhost';
    $config['x']
}
"#;
    let cursor_offset = content.find("$config['x']").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$config",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $config to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("db: array{host: array{primary: string}}"),
        "Shape should contain deeply nested keys, got: {ts}"
    );
}

/// Mixed single-level and nested key assignments should merge correctly.
#[test]
fn resolve_var_shape_mixed_single_and_nested_keys() {
    let content = r#"<?php
function test() {
    $data['name'] = 'John';
    $data['address']['city'] = 'NYC';
    $data['address']['zip'] = '10001';
    $data['x']
}
"#;
    let cursor_offset = content.find("$data['x']").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$data",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $data to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("name: string"),
        "Shape should contain 'name: string', got: {ts}"
    );
    assert!(
        ts.contains("city: string"),
        "Shape should contain nested 'city: string', got: {ts}"
    );
    assert!(
        ts.contains("zip: string"),
        "Shape should contain nested 'zip: string', got: {ts}"
    );
}

/// `array_sum` should resolve to `int|float`.
#[test]
fn resolve_var_array_sum() {
    let content = r#"<?php
function test() {
    $result = array_sum([10, 20, 30]);
    echo $result;
}
"#;
    let cursor_offset = content.find("echo $result").unwrap() as u32;

    // Provide a function loader that returns FunctionInfo with the
    // stub return type (int|float), matching what the real backend
    // produces from phpstorm-stubs.
    let func_loader = |name: &str, _offset: u32| -> Option<crate::types::FunctionInfo> {
        if name.eq_ignore_ascii_case("array_sum") || name.eq_ignore_ascii_case("array_product") {
            Some(stub_function_info(
                name,
                Some(PhpType::union(vec![PhpType::int(), PhpType::float()])),
            ))
        } else {
            None
        }
    };

    let results = super::resolve_variable_types(
        "$result",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders {
            function_loader: Some(&func_loader),
            ..Loaders::default()
        },
    );

    assert!(!results.is_empty(), "Should resolve $result to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("int") && ts.contains("float"),
        "array_sum should return int|float, got: {ts}"
    );
}

/// `array_product` should resolve to `int|float`.
#[test]
fn resolve_var_array_product() {
    let content = r#"<?php
function test() {
    $result = array_product([2, 3, 4]);
    echo $result;
}
"#;
    let cursor_offset = content.find("echo $result").unwrap() as u32;

    let func_loader = |name: &str, _offset: u32| -> Option<crate::types::FunctionInfo> {
        if name.eq_ignore_ascii_case("array_sum") || name.eq_ignore_ascii_case("array_product") {
            Some(stub_function_info(
                name,
                Some(PhpType::union(vec![PhpType::int(), PhpType::float()])),
            ))
        } else {
            None
        }
    };

    let results = super::resolve_variable_types(
        "$result",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders {
            function_loader: Some(&func_loader),
            ..Loaders::default()
        },
    );

    assert!(!results.is_empty(), "Should resolve $result to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("int") && ts.contains("float"),
        "array_product should return int|float, got: {ts}"
    );
}

#[test]
fn resolve_var_after_by_ref_closure_capture_call() {
    let content = r#"<?php
function c(callable $x) {
    $called = $x();

    return $called;
}

$foo = null;

c(function () use (&$foo) {
    return $foo = 1;
});

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "1");
}

#[test]
fn resolve_var_after_by_ref_closure_capture_call_in_namespace() {
    let content = r#"<?php
namespace App;

function c(callable $x) {
    $called = $x();

    return $called;
}

$foo = null;

c(function () use (&$foo) {
    return $foo = 1;
});

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "1");
}

#[test]
fn resolve_var_after_by_ref_closure_capture_assignment_statement() {
    let content = r#"<?php
function c(callable $x) {
    $called = $x();

    return $called;
}

$foo = null;

c(function () use (&$foo) {
    $foo = 1;
});

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "1");
}

#[test]
fn by_ref_closure_capture_widens_when_callable_not_invoked_immediately() {
    // A later-invoked callable may run zero or more times at any later
    // point, so the capture's assigned type is unioned with the outer
    // type rather than replacing it.
    let content = r#"<?php
/** @param-later-invoked-callable $x */
function c(callable $x) {
    $x();
}

$foo = null;

c(function () use (&$foo) {
    return $foo = 1;
});

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "null|1");
}

#[test]
fn by_ref_closure_capture_widens_for_method_by_default() {
    // Method callable parameters are later-invoked by default, so the
    // capture widens the outer type instead of replacing it.
    let content = r#"<?php
class A {
    public function c(callable $x) {
        $x();
    }
}

$a = new A();
$foo = null;

$a->c(function () use (&$foo) {
    $foo = 1;
});

$foo;
"#;
    let a_class = make_class("A");
    let all_classes: Vec<Arc<ClassInfo>> = vec![Arc::new(a_class.clone())];
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "A" {
            Some(Arc::new(a_class.clone()))
        } else {
            None
        }
    };
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "null|1");
}

#[test]
fn by_ref_closure_capture_propagates_for_immediately_invoked_method_callable() {
    let content = r#"<?php
class A {
    /** @param-immediately-invoked-callable $x */
    public function c(callable $x) {}
}

$a = new A();
$foo = null;

$a->c(function () use (&$foo) {
    $foo = 1;
});

$foo;
"#;
    let a_class = make_class("A");
    let all_classes: Vec<Arc<ClassInfo>> = vec![Arc::new(a_class.clone())];
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "A" {
            Some(Arc::new(a_class.clone()))
        } else {
            None
        }
    };
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "1");
}

#[test]
fn by_ref_closure_capture_propagates_for_immediately_invoked_method_callable_in_namespace() {
    let content = r#"<?php
namespace App;

class A {
    /** @param-immediately-invoked-callable $x */
    public function c(callable $x) {}
}

$a = new A();
$foo = null;

$a->c(function () use (&$foo) {
    $foo = 1;
});

$foo;
"#;
    let a_class = make_class("A");
    let all_classes: Vec<Arc<ClassInfo>> = vec![Arc::new(a_class.clone())];
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "A" || name == "App\\A" {
            Some(Arc::new(a_class.clone()))
        } else {
            None
        }
    };
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "1");
}

#[test]
fn by_ref_closure_capture_matches_named_argument_by_name_for_function() {
    // `$b` is stored (later-invoked), not called immediately. Passing the
    // closure as the named argument `b:` places it in the first argument
    // slot, but it must still be matched to `$b` (not `$a`) so its
    // later-invoked tag widens the outer type instead of replacing it
    // (a wrong match to the untagged `$a` would replace, giving `1`).
    let content = r#"<?php
/** @param-later-invoked-callable $b */
function c(callable $a, callable $b) {
    $a();
}

$foo = null;

c(b: function () use (&$foo) {
    $foo = 1;
}, a: function () {});

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "null|1");
}

#[test]
fn by_ref_closure_capture_matches_named_argument_by_name_for_method() {
    // Only `$b` is immediately invoked. Passing the closure as the named
    // argument `b:` puts it in the first argument slot; it must still be
    // matched to `$b` so its immediately-invoked tag propagates the
    // capture.
    let content = r#"<?php
class A {
    /** @param-immediately-invoked-callable $b */
    public function c(callable $a, callable $b) {}
}

$a = new A();
$foo = null;

$a->c(b: function () use (&$foo) {
    $foo = 1;
});

$foo;
"#;
    let a_class = make_class("A");
    let all_classes: Vec<Arc<ClassInfo>> = vec![Arc::new(a_class.clone())];
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "A" {
            Some(Arc::new(a_class.clone()))
        } else {
            None
        }
    };
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "1");
}

#[test]
fn by_ref_closure_capture_widens_for_unresolvable_method_receiver() {
    // The receiver of `transaction()` resolves through a chained static
    // call whose class may be unknown.  The closure could still run, so
    // the capture's assigned type must widen the outer null rather than
    // being ignored entirely.
    let content = r#"<?php
$foo = null;

\Some\Unknown\Db::connection()->transaction(function () use (&$foo) {
    $foo = 'string';
});

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "null|'string'");
}

#[test]
fn by_ref_closure_capture_widens_with_conditional_assignment_inside() {
    // The assignment inside the closure sits in an `if` branch; the
    // merged closure exit state (`null|'string'`) still reaches the
    // outer scope.
    let content = r#"<?php
$foo = null;

\Some\Unknown\Helper::run(function () use (&$foo) {
    $asset = 'object';
    if ($asset !== null) {
        $foo = 'string';
    }
});

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("null") && ts.contains("'string'"),
        "Expected null|'string', got: {ts}"
    );
}

#[test]
fn by_ref_closure_capture_propagates_for_immediately_invoked_closure_expression() {
    // An IIFE runs before the statement completes, so the closure's
    // final state replaces the outer type.
    let content = r#"<?php
$foo = null;

(function () use (&$foo) {
    $foo = 1;
})();

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "1");
}

#[test]
fn by_ref_closure_capture_widens_when_closure_stored_in_variable() {
    // A closure assigned to a variable may run later (PHPStan widens at
    // the definition, not the call), so the outer type becomes a union.
    let content = r#"<?php
$foo = null;

$fn = function () use (&$foo) {
    $foo = 1;
};
$fn();

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "null|1");
}

#[test]
fn by_ref_closure_capture_widens_when_closure_sits_in_a_chain_receiver() {
    // The closure is an argument of the *receiver* of the outer call,
    // not of the outer call itself, so it is only reached by descending
    // into the receiver expression before the argument list.
    let content = r#"<?php
$foo = null;

\Some\Unknown\Db::query(function () use (&$foo) {
    $foo = 1;
})->run();

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "null|1");
}

#[test]
fn by_ref_closure_capture_widens_when_closure_is_nested_in_another_argument() {
    // The closure is an argument of a call that is itself an argument,
    // so reaching it means descending through the outer argument list.
    let content = r#"<?php
$foo = null;

outer(inner(function () use (&$foo) {
    $foo = 1;
}));

$foo;
"#;
    let cursor_offset = content.rfind("$foo;").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$foo",
        &ClassInfo::default(),
        &[],
        content,
        cursor_offset,
        &|_| None,
        None,
        Loaders::default(),
    );

    assert!(!results.is_empty(), "Should resolve $foo to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert_eq!(ts, "null|1");
}

/// `array_reduce` with a class initial value should resolve to that class.
#[test]
fn resolve_var_array_reduce_initial_value() {
    let content = r#"<?php
class Accumulator { public function total(): int { return 0; } }
function test() {
    $result = array_reduce([1, 2, 3], function(Accumulator $carry, int $item): Accumulator {
        return $carry;
    }, new Accumulator());
    $result->
}
"#;
    let acc = make_class("Accumulator");
    let all_classes: Vec<Arc<ClassInfo>> = vec![Arc::new(acc.clone())];
    let class_loader = move |name: &str| -> Option<Arc<ClassInfo>> {
        if name == "Accumulator" {
            Some(Arc::new(make_class("Accumulator")))
        } else {
            None
        }
    };

    // Provide a function loader that returns array_reduce with
    // @template TCarry, @param TCarry $initial, @return TCarry
    // (matching what the real backend parses from the upstream stubs).
    let func_loader = |name: &str, _offset: u32| -> Option<crate::types::FunctionInfo> {
        if name.eq_ignore_ascii_case("array_reduce") {
            let mut fi = stub_function_info(name, Some(PhpType::named(atom("TCarry"))));
            fi.parameters = vec![
                crate::test_fixtures::make_param("$array", Some("array"), true),
                crate::test_fixtures::make_param("$callback", Some("callable"), true),
                crate::test_fixtures::make_param("$initial", Some("TCarry"), false),
            ]
            .into();
            fi.template_params = vec![crate::atom::atom("TCarry"), crate::atom::atom("TValue")];
            fi.template_bindings =
                vec![(crate::atom::atom("TCarry"), crate::atom::atom("$initial"))];
            Some(fi)
        } else {
            None
        }
    };

    let cursor_offset = content.find("$result->").unwrap() as u32;

    let results = super::resolve_variable_types(
        "$result",
        &ClassInfo::default(),
        &all_classes,
        content,
        cursor_offset,
        &class_loader,
        None,
        Loaders {
            function_loader: Some(&func_loader),
            ..Loaders::default()
        },
    );

    assert!(!results.is_empty(), "Should resolve $result to a type");
    let ts = ResolvedType::types_joined(&results).to_string();
    assert!(
        ts.contains("Accumulator"),
        "array_reduce should return type of initial value, got: {ts}"
    );
}

/// Helper: build a minimal `FunctionInfo` with a given name and return type,
/// simulating what the real backend produces from phpstorm-stubs.
fn stub_function_info(name: &str, return_type: Option<PhpType>) -> crate::types::FunctionInfo {
    crate::types::FunctionInfo {
        name: crate::atom::atom(name),
        name_offset: 0,
        parameters: Vec::new().into(),
        return_type,
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
        template_params: Vec::new(),
        template_bindings: Vec::new(),
        template_param_bounds: Default::default(),
        throws: Vec::new(),
        is_polyfill: false,
        overloads: vec![],
    }
}
