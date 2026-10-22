use crate::common::{create_test_backend, goto_definition_at, open_php};
use tower_lsp::lsp_types::*;

// ─── Tests: Stored byte offsets and offset_to_position ──────────────────────

/// Verify that every MethodInfo, PropertyInfo, and ConstantInfo created from
/// a real PHP parse has `name_offset > 0`.
#[tokio::test]
async fn test_parsed_members_have_nonzero_name_offset() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_offsets.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    const GREETING = 'Hello';\n",
        "    public string $name = 'World';\n",
        "    public static int $count = 0;\n",
        "\n",
        "    public function greet(): string {\n",
        "        return self::GREETING . ' ' . $this->name;\n",
        "    }\n",
        "\n",
        "    private static function increment(): void {\n",
        "        self::$count++;\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");
    assert_eq!(classes.len(), 1);
    let cls = &classes[0];
    assert_eq!(cls.name, "Greeter");

    // All methods should have name_offset > 0
    for method in &cls.methods {
        assert!(
            method.name_offset > 0,
            "Method '{}' should have name_offset > 0, got {}",
            method.name,
            method.name_offset
        );
    }

    // All properties should have name_offset > 0
    for prop in &cls.properties {
        assert!(
            prop.name_offset > 0,
            "Property '{}' should have name_offset > 0, got {}",
            prop.name,
            prop.name_offset
        );
    }

    // All constants should have name_offset > 0
    for constant in &cls.constants {
        assert!(
            constant.name_offset > 0,
            "Constant '{}' should have name_offset > 0, got {}",
            constant.name,
            constant.name_offset
        );
    }
}

/// Verify that keyword_offset is populated for all class-like kinds.
#[tokio::test]
async fn test_keyword_offset_populated_for_all_class_kinds() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_keyword_offsets.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MyClass {\n",
        "    public function foo(): void {}\n",
        "}\n",
        "\n",
        "interface MyInterface {\n",
        "    public function bar(): void;\n",
        "}\n",
        "\n",
        "trait MyTrait {\n",
        "    public function baz(): void {}\n",
        "}\n",
        "\n",
        "enum MyEnum {\n",
        "    case Alpha;\n",
        "    case Beta;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");

    assert!(classes.len() >= 4, "Expected 4 class-like declarations");

    for cls in &classes {
        assert!(
            cls.keyword_offset > 0,
            "{:?} '{}' should have keyword_offset > 0, got {}",
            cls.kind,
            cls.name,
            cls.keyword_offset
        );
    }

    // Verify each points to the right keyword in the source
    let class_info = classes.iter().find(|c| c.name == "MyClass").unwrap();
    let kw_byte = class_info.keyword_offset as usize;
    assert_eq!(
        &text[kw_byte..kw_byte + 5],
        "class",
        "keyword_offset for MyClass should point to 'class'"
    );

    let iface_info = classes.iter().find(|c| c.name == "MyInterface").unwrap();
    let kw_byte = iface_info.keyword_offset as usize;
    assert_eq!(
        &text[kw_byte..kw_byte + 9],
        "interface",
        "keyword_offset for MyInterface should point to 'interface'"
    );

    let trait_info = classes.iter().find(|c| c.name == "MyTrait").unwrap();
    let kw_byte = trait_info.keyword_offset as usize;
    assert_eq!(
        &text[kw_byte..kw_byte + 5],
        "trait",
        "keyword_offset for MyTrait should point to 'trait'"
    );

    let enum_info = classes.iter().find(|c| c.name == "MyEnum").unwrap();
    let kw_byte = enum_info.keyword_offset as usize;
    assert_eq!(
        &text[kw_byte..kw_byte + 4],
        "enum",
        "keyword_offset for MyEnum should point to 'enum'"
    );
}

/// Verify that name_offset for methods, properties, and constants points to
/// the correct token in the source text.
#[tokio::test]
async fn test_name_offset_points_to_correct_token() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_name_tokens.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Demo {\n",
        "    const STATUS_ACTIVE = 1;\n",
        "    public string $title = '';\n",
        "\n",
        "    public function doSomething(): void {}\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");
    let cls = &classes[0];

    // Check method name offset
    let method = cls
        .methods
        .iter()
        .find(|m| m.name == "doSomething")
        .unwrap();
    let off = method.name_offset as usize;
    assert_eq!(
        &text[off..off + "doSomething".len()],
        "doSomething",
        "Method name_offset should point to 'doSomething'"
    );

    // Check constant name offset
    let constant = cls
        .constants
        .iter()
        .find(|c| c.name == "STATUS_ACTIVE")
        .unwrap();
    let off = constant.name_offset as usize;
    assert_eq!(
        &text[off..off + "STATUS_ACTIVE".len()],
        "STATUS_ACTIVE",
        "Constant name_offset should point to 'STATUS_ACTIVE'"
    );

    // Check property name offset — points to the `$` of `$title`
    let prop = cls.properties.iter().find(|p| p.name == "title").unwrap();
    let off = prop.name_offset as usize;
    assert_eq!(
        &text[off..off + "$title".len()],
        "$title",
        "Property name_offset should point to '$title'"
    );
}

/// Verify that enum case name_offset points to the case name token.
#[tokio::test]
async fn test_enum_case_name_offset() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_enum_case_offset.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Color {\n",
        "    case Red;\n",
        "    case Green;\n",
        "    case Blue;\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");
    let cls = &classes[0];

    for constant in &cls.constants {
        let off = constant.name_offset as usize;
        let name_len = constant.name.len();
        assert_eq!(
            &text[off..off + name_len],
            constant.name,
            "Enum case '{}' name_offset should point to its name",
            constant.name
        );
    }
}

/// Verify that promoted constructor properties have correct name_offset.
#[tokio::test]
async fn test_promoted_property_name_offset() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_promoted.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Point {\n",
        "    public function __construct(\n",
        "        public readonly float $x,\n",
        "        public readonly float $y,\n",
        "        private string $label = 'origin',\n",
        "    ) {}\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");
    let cls = &classes[0];

    // Should have 3 promoted properties: x, y, label
    assert!(
        cls.properties.len() >= 3,
        "Expected at least 3 promoted properties, got {}",
        cls.properties.len()
    );

    for prop in &cls.properties {
        assert!(
            prop.name_offset > 0,
            "Promoted property '{}' should have name_offset > 0",
            prop.name
        );
        // The offset should point to `$` + name in the source
        let off = prop.name_offset as usize;
        let expected = format!("${}", prop.name);
        assert_eq!(
            &text[off..off + expected.len()],
            expected,
            "Promoted property '{}' name_offset should point to '{}' in source",
            prop.name,
            expected
        );
    }
}

/// Verify that offset_to_position produces the same line/character as the
/// old text-search approach for a representative class.
#[tokio::test]
async fn test_offset_to_position_matches_text_search() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_offset_pos.php").unwrap();
    let text = concat!(
        "<?php\n",
        "abstract class AbstractHandler {\n",
        "    const MAX_RETRIES = 3;\n",
        "    protected int $retryCount = 0;\n",
        "\n",
        "    abstract public function handle(): void;\n",
        "\n",
        "    final public function retry(): bool {\n",
        "        return $this->retryCount < self::MAX_RETRIES;\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Click on "MAX_RETRIES" in `self::MAX_RETRIES` on line 8
    let result = goto_definition_at(&backend, &uri, 8, 45).await;
    assert!(
        result.is_some(),
        "Should resolve self::MAX_RETRIES to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            // "const MAX_RETRIES" is on line 2
            assert_eq!(
                location.range.start.line, 2,
                "MAX_RETRIES should be found on line 2"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }

    // Also test method go-to-definition

    // Place cursor on "retryCount" in `$this->retryCount` on line 8
    let result = goto_definition_at(&backend, &uri, 8, 25).await;
    assert!(
        result.is_some(),
        "Should resolve $this->retryCount to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            // "protected int $retryCount" is on line 3
            assert_eq!(
                location.range.start.line, 3,
                "retryCount property should be found on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Verify that a class name inside a string literal does NOT cause a false
/// match when using the AST-driven keyword_offset path.
///
/// The old text-search approach would match `class Foo` inside a string
/// like `"class Foo {...}"`, but the AST-based approach only stores offsets
/// for actual declarations.
#[tokio::test]
async fn test_class_name_inside_string_does_not_false_match() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_string_false_match.php").unwrap();
    // The string literal on line 3 contains "class Greeter" — this should
    // NOT be treated as a class definition.
    let text = concat!(
        "<?php\n",
        "class Renderer {\n",
        "    public function template(): string {\n",
        "        return \"class Greeter { public function greet() {} }\";\n",
        "    }\n",
        "}\n",
        "\n",
        "class Greeter {\n",
        "    public function greet(): string {\n",
        "        return 'hello';\n",
        "    }\n",
        "}\n",
        "\n",
        "function main(): void {\n",
        "    $g = new Greeter();\n",
        "    $g->greet();\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // The real `class Greeter` is on line 7, NOT line 3 (which is inside
    // a string).  Verify keyword_offset points to line 7.
    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");

    let greeter = classes.iter().find(|c| c.name == "Greeter").unwrap();
    assert!(
        greeter.keyword_offset > 0,
        "Greeter should have keyword_offset > 0"
    );

    // Count newlines before keyword_offset to determine the line
    let kw_off = greeter.keyword_offset as usize;
    let line_num = text[..kw_off].matches('\n').count();
    assert_eq!(
        line_num, 7,
        "keyword_offset should point to line 7 (the real class Greeter), not to the string literal on line 3"
    );

    // Verify go-to-definition for `new Greeter()` on line 14 lands on line 7
    let result = goto_definition_at(&backend, &uri, 14, 14).await;
    assert!(
        result.is_some(),
        "Should resolve Greeter to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 7,
                "Greeter definition should be on line 7, not inside the string on line 3"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Verify that a method name mentioned inside a heredoc does NOT interfere
/// with go-to-definition via the AST offset path.
#[tokio::test]
async fn test_member_name_in_heredoc_does_not_false_match() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_heredoc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function write(string $msg): void {}\n",
        "\n",
        "    public function template(): string {\n",
        "        return <<<EOT\n",
        "function write() { /* not a real declaration */ }\n",
        "EOT;\n",
        "    }\n",
        "}\n",
        "\n",
        "function main(): void {\n",
        "    $logger = new Logger();\n",
        "    $logger->write('test');\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Click on "write" in `$logger->write('test')` on line 13
    let result = goto_definition_at(&backend, &uri, 13, 15).await;
    assert!(
        result.is_some(),
        "Should resolve $logger->write() to its declaration"
    );

    match result.unwrap() {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            // The real `public function write(...)` is on line 2, not line 6
            assert_eq!(
                location.range.start.line, 2,
                "write() should resolve to line 2, not to the heredoc on line 6"
            );
        }
        other => panic!("Expected Scalar location, got: {:?}", other),
    }
}

/// Verify that go-to-definition works for a trait member via $this.
#[tokio::test]
async fn test_trait_member_offset() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_trait_offset.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Timestampable {\n",
        "    public string $createdAt = '';\n",
        "    public function touch(): void {}\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");
    let trait_info = classes.iter().find(|c| c.name == "Timestampable").unwrap();

    // Trait keyword_offset should point to "trait"
    let kw = trait_info.keyword_offset as usize;
    assert_eq!(&text[kw..kw + 5], "trait");

    // Method name_offset should point to "touch"
    let method = trait_info
        .methods
        .iter()
        .find(|m| m.name == "touch")
        .unwrap();
    let off = method.name_offset as usize;
    assert_eq!(&text[off..off + 5], "touch");

    // Property name_offset should point to "$createdAt"
    let prop = trait_info
        .properties
        .iter()
        .find(|p| p.name == "createdAt")
        .unwrap();
    let off = prop.name_offset as usize;
    assert_eq!(&text[off..off + 10], "$createdAt");
}

/// Verify that name_offset works correctly for a final class with comments,
/// heredocs, and string mentions of class names — a comprehensive stress test.
#[tokio::test]
async fn test_offsets_with_comments_heredocs_and_strings() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_stress.php").unwrap();
    let text = concat!(
        "<?php\n",
        "// class FakeClass { const FAKE = 1; }\n",
        "/* class AnotherFake {} */\n",
        "\n",
        "final class RealClass {\n",
        "    /** The status constant */\n",
        "    const STATUS = 'active';\n",
        "\n",
        "    // $fakeProperty is not real\n",
        "    public string $realProp = 'value';\n",
        "\n",
        "    /**\n",
        "     * Does something.\n",
        "     * function fakeMethod() {}\n",
        "     */\n",
        "    public function realMethod(): string {\n",
        "        $str = 'function realMethod() { const STATUS = 0; }';\n",
        "        return $str;\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");

    // Only RealClass should be extracted — FakeClass and AnotherFake are
    // inside comments and should not appear.
    assert_eq!(classes.len(), 1, "Only RealClass should be parsed");
    let cls = &classes[0];
    assert_eq!(cls.name, "RealClass");

    // keyword_offset should point to "class" (the `final` modifier is
    // separate — the keyword token is `class` itself).
    let kw = cls.keyword_offset as usize;
    assert_eq!(&text[kw..kw + 5], "class");

    // Verify method offset
    let method = cls.methods.iter().find(|m| m.name == "realMethod").unwrap();
    let off = method.name_offset as usize;
    assert_eq!(&text[off..off + "realMethod".len()], "realMethod");

    // Verify constant offset
    let constant = cls.constants.iter().find(|c| c.name == "STATUS").unwrap();
    let off = constant.name_offset as usize;
    assert_eq!(&text[off..off + "STATUS".len()], "STATUS");

    // Verify property offset
    let prop = cls
        .properties
        .iter()
        .find(|p| p.name == "realProp")
        .unwrap();
    let off = prop.name_offset as usize;
    assert_eq!(&text[off..off + "$realProp".len()], "$realProp");
}

/// Verify that backed enum cases have correct name_offset.
#[tokio::test]
async fn test_backed_enum_case_offsets() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_backed_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "enum Suit: string {\n",
        "    case Hearts = 'H';\n",
        "    case Diamonds = 'D';\n",
        "    case Clubs = 'C';\n",
        "    case Spades = 'S';\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");
    let cls = &classes[0];
    assert_eq!(cls.name, "Suit");

    let expected_cases = ["Hearts", "Diamonds", "Clubs", "Spades"];
    for case_name in &expected_cases {
        let constant = cls
            .constants
            .iter()
            .find(|c| c.name == *case_name)
            .unwrap_or_else(|| panic!("Should find case '{}'", case_name));

        let off = constant.name_offset as usize;
        assert_eq!(
            &text[off..off + case_name.len()],
            *case_name,
            "Enum case '{}' name_offset should point to its name",
            case_name
        );
    }
}

/// Verify that go-to-definition for self::CONSTANT uses the offset path
/// and resolves to the right line (interface context).
#[tokio::test]
async fn test_interface_constant_definition_via_offset() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_iface_const.php").unwrap();
    let text = concat!(
        "<?php\n",
        "interface Configurable {\n",
        "    const VERSION = '1.0';\n",
        "    public function configure(): void;\n",
        "}\n",
        "\n",
        "class App implements Configurable {\n",
        "    public function configure(): void {\n",
        "        echo self::VERSION;\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");

    let iface = classes.iter().find(|c| c.name == "Configurable").unwrap();
    let kw = iface.keyword_offset as usize;
    assert_eq!(&text[kw..kw + 9], "interface");

    let version_const = iface
        .constants
        .iter()
        .find(|c| c.name == "VERSION")
        .unwrap();
    let off = version_const.name_offset as usize;
    assert_eq!(&text[off..off + 7], "VERSION");
}

/// Verify anonymous classes do NOT have keyword_offset set (they use 0).
#[tokio::test]
async fn test_anonymous_class_keyword_offset_is_zero() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///test_anon_class.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Outer {\n",
        "    public function makeAnon(): object {\n",
        "        return new class {\n",
        "            public function inner(): string {\n",
        "                return 'anon';\n",
        "            }\n",
        "        };\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let classes = backend
        .get_classes_for_uri(uri.as_str())
        .expect("Should have parsed classes");

    let outer = classes.iter().find(|c| c.name == "Outer").unwrap();
    assert!(
        outer.keyword_offset > 0,
        "Named class Outer should have keyword_offset > 0"
    );

    let anon = classes
        .iter()
        .find(|c| c.name.starts_with("__anonymous@"))
        .unwrap();
    assert_eq!(
        anon.keyword_offset, 0,
        "Anonymous class should have keyword_offset == 0"
    );

    // But the anonymous class's methods should still have valid name_offsets
    let inner_method = anon.methods.iter().find(|m| m.name == "inner").unwrap();
    assert!(
        inner_method.name_offset > 0,
        "Anonymous class method 'inner' should have name_offset > 0"
    );
    let off = inner_method.name_offset as usize;
    assert_eq!(&text[off..off + 5], "inner");
}

/// Ctrl+Click on a function's own name at its declaration.
///
/// There is nowhere to jump to -- resolving the name would land on the
/// line the cursor already sits on -- so the answer is the declaration
/// itself, which is what a class, member, and namespace declaration
/// answer and what an editor turns into a usage list.  Before, the name
/// went through the call path and the click did nothing at all.
#[tokio::test]
async fn test_goto_definition_on_a_function_declaration_offers_usages() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///function_declaration.php").unwrap();
    let content = "<?php\nfunction helper(): void {}\n\nhelper();\n";

    open_php(&backend, &uri, content).await;

    // Click on "helper" in `function helper(): void {}` on line 1.
    let result = goto_definition_at(&backend, &uri, 1, 11)
        .await
        .expect("a function declaration should answer with itself");

    match result {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri, uri);
            assert_eq!(
                location.range.start.line, 1,
                "the answer is the declaration the cursor is on, got {:?}",
                location.range
            );
        }
        GotoDefinitionResponse::Array(locations) => {
            assert_eq!(locations.len(), 1, "got {:?}", locations);
            assert_eq!(locations[0].range.start.line, 1, "got {:?}", locations);
        }
        other => panic!("Expected a location, got: {:?}", other),
    }
}
