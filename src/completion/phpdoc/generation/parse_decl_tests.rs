use super::*;
use crate::completion::phpdoc::context::DocblockContext;

// ── Declaration classification ──────────────────────────────────────

#[test]
fn classifies_function() {
    let decl = "function test(string $name): void {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::FunctionOrMethod
    ));
}

#[test]
fn classifies_method() {
    let decl = "    public function test(): int {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::FunctionOrMethod
    ));
}

#[test]
fn classifies_abstract_method() {
    let decl = "    abstract public function test(): int;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::FunctionOrMethod
    ));
}

#[test]
fn classifies_class() {
    let decl = "class Foo extends Bar {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_abstract_class() {
    let decl = "abstract class Foo {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_interface() {
    let decl = "interface Foo {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_trait() {
    let decl = "trait Foo {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_enum() {
    let decl = "enum Status: string {}";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::ClassLike
    ));
}

#[test]
fn classifies_property() {
    let decl = "    public string $name;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Property
    ));
}

#[test]
fn classifies_untyped_property() {
    let decl = "    public $name;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Property
    ));
}

#[test]
fn classifies_constant() {
    let decl = "    const FOO = 'bar';";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Constant
    ));
}

#[test]
fn classifies_public_constant() {
    let decl = "    public const int MAX = 100;";
    assert!(matches!(
        classify_declaration(decl),
        DocblockContext::Constant
    ));
}

// ── Parameter parsing ───────────────────────────────────────────────

#[test]
fn parses_params_with_types() {
    let text = "function test(string $name, int $age): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 2);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("string")), "$name".to_string())
    );
    assert_eq!(
        info.params[1],
        (Some(PhpType::parse("int")), "$age".to_string())
    );
    assert_eq!(info.return_type, Some(PhpType::parse("void")));
}

#[test]
fn parses_params_without_types() {
    let text = "function test($name, $age) {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 2);
    assert_eq!(info.params[0], (None, "$name".to_string()));
    assert_eq!(info.params[1], (None, "$age".to_string()));
}

#[test]
fn parses_nullable_type() {
    let text = "function test(?string $name): ?int {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params[0].0, Some(PhpType::parse("?string")));
    assert_eq!(info.return_type, Some(PhpType::parse("?int")));
}

#[test]
fn parses_union_type() {
    let text = "function test(string|int $value): string|false {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params[0].0, Some(PhpType::parse("string|int")));
    assert_eq!(info.return_type, Some(PhpType::parse("string|false")));
}

#[test]
fn parses_variadic_param() {
    let text = "function test(string ...$names): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("string")), "$names".to_string())
    );
}

#[test]
fn parses_reference_param() {
    let text = "function test(array &$data): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("array")), "$data".to_string())
    );
}

#[test]
fn parses_param_with_default() {
    let text = "function test(string $name = 'world'): void {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("string")), "$name".to_string())
    );
}

#[test]
fn parses_no_params() {
    let text = "function test(): void {}";
    let info = parse_declaration_info(text);
    assert!(info.params.is_empty());
    assert_eq!(info.return_type, Some(PhpType::parse("void")));
}

#[test]
fn parses_property_type() {
    let text = "    public string $name;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn parses_readonly_property_type() {
    let text = "    public readonly string $name;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn parses_typed_constant_extracts_only_type() {
    let text = "    const int COW = 0;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("int")));
}

#[test]
fn parses_public_typed_constant_extracts_only_type() {
    let text = "    public const string NAME = 'foo';";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, Some(PhpType::parse("string")));
}

#[test]
fn parses_untyped_constant_has_no_type() {
    let text = "    const MAX = 100;";
    let info = parse_declaration_info(text);
    assert_eq!(info.type_hint, None);
}

#[test]
fn parses_promoted_param_type() {
    let text = "function __construct(public readonly bool $selected) {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.params.len(), 1);
    assert_eq!(
        info.params[0],
        (Some(PhpType::parse("bool")), "$selected".to_string())
    );
}

#[test]
fn parses_class_extends() {
    let text = "class Child extends Base {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.extends_names, vec!["Base"]);
    assert!(info.implements_names.is_empty());
}

#[test]
fn parses_class_implements() {
    let text = "class Foo implements Bar, Baz {}";
    let info = parse_declaration_info(text);
    assert!(info.extends_names.is_empty());
    assert_eq!(info.implements_names, vec!["Bar", "Baz"]);
}

#[test]
fn parses_class_extends_and_implements() {
    let text = "class Child extends Base implements Iface {}";
    let info = parse_declaration_info(text);
    assert_eq!(info.extends_names, vec!["Base"]);
    assert_eq!(info.implements_names, vec!["Iface"]);
}

// ── is_class_like_keyword ───────────────────────────────────────────

#[test]
fn is_class_like_plain_class() {
    assert!(is_class_like_keyword("class Foo {}"));
}

#[test]
fn is_class_like_abstract_class() {
    assert!(is_class_like_keyword("abstract class Foo {}"));
}

#[test]
fn is_class_like_interface() {
    assert!(is_class_like_keyword("interface Foo {}"));
}

#[test]
fn is_class_like_not_function() {
    assert!(!is_class_like_keyword("function foo() {}"));
}

#[test]
fn is_class_like_not_property() {
    assert!(!is_class_like_keyword("public string $foo;"));
}

// ── extract_class_supertypes ────────────────────────────────────────

#[test]
fn extracts_extends_from_decl() {
    let (parents, ifaces) = extract_class_supertypes("class Child extends Base {}");
    assert_eq!(parents, vec!["Base"]);
    assert!(ifaces.is_empty());
}

#[test]
fn extracts_implements_from_decl() {
    let (parents, ifaces) = extract_class_supertypes("class Foo implements Bar, Baz {}");
    assert!(parents.is_empty());
    assert_eq!(ifaces, vec!["Bar", "Baz"]);
}

#[test]
fn extracts_both_from_decl() {
    let (parents, ifaces) =
        extract_class_supertypes("class Child extends Base implements Iface {}");
    assert_eq!(parents, vec!["Base"]);
    assert_eq!(ifaces, vec!["Iface"]);
}

// ── classify_declaration: variable assignments ──────────────────────

#[test]
fn classifies_variable_assignment() {
    let ctx = classify_declaration("    $items = [''];\n");
    assert!(matches!(ctx, DocblockContext::Inline));
}

#[test]
fn classifies_variable_assignment_no_value() {
    let ctx = classify_declaration("    $x = null;\n");
    assert!(matches!(ctx, DocblockContext::Inline));
}

#[test]
fn classifies_variable_not_confused_with_property() {
    // Properties have modifiers; bare `$var` does not.
    let ctx = classify_declaration("    public string $name;\n");
    assert!(matches!(ctx, DocblockContext::Property));
}

#[test]
fn extracts_variable_name_from_inline_assignment() {
    let info = parse_declaration_info("    $items = [''];\n");
    assert_eq!(info.variable_name.as_deref(), Some("$items"));
}

#[test]
fn extracts_variable_name_from_simple_assignment() {
    let info = parse_declaration_info("    $count = 42;\n");
    assert_eq!(info.variable_name.as_deref(), Some("$count"));
}
