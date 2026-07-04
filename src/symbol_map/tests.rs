use super::docblock::is_navigable_type;
use super::extraction::extract_symbol_map;
use super::*;

// ── SymbolMap::lookup tests ─────────────────────────────────────────

fn make_span(start: u32, end: u32, name: &str) -> SymbolSpan {
    SymbolSpan {
        start,
        end,
        kind: SymbolKind::ClassReference {
            name: name.to_string(),
            is_fqn: false,
            context: ClassRefContext::Other,
        },
    }
}

#[test]
fn lookup_empty_map_returns_none() {
    let map = SymbolMap::default();
    assert!(map.lookup(0).is_none());
    assert!(map.lookup(100).is_none());
}

#[test]
fn lookup_hit_at_start() {
    let map = SymbolMap {
        spans: vec![make_span(10, 15, "Foo")],
        ..Default::default()
    };
    assert!(map.lookup(10).is_some());
    assert_eq!(map.lookup(10).unwrap().start, 10);
}

#[test]
fn lookup_hit_at_end_minus_one() {
    let map = SymbolMap {
        spans: vec![make_span(10, 15, "Foo")],
        ..Default::default()
    };
    assert!(map.lookup(14).is_some());
}

#[test]
fn lookup_miss_at_end() {
    let map = SymbolMap {
        spans: vec![make_span(10, 15, "Foo")],
        ..Default::default()
    };
    assert!(map.lookup(15).is_none());
}

#[test]
fn lookup_miss_before_first_span() {
    let map = SymbolMap {
        spans: vec![make_span(10, 15, "Foo")],
        ..Default::default()
    };
    assert!(map.lookup(5).is_none());
}

#[test]
fn lookup_miss_in_gap() {
    let map = SymbolMap {
        spans: vec![make_span(10, 15, "Foo"), make_span(20, 25, "Bar")],
        ..Default::default()
    };
    assert!(map.lookup(17).is_none());
}

#[test]
fn lookup_correct_span_in_sequence() {
    let map = SymbolMap {
        spans: vec![
            make_span(10, 15, "Foo"),
            make_span(20, 25, "Bar"),
            make_span(30, 35, "Baz"),
        ],
        ..Default::default()
    };
    let result = map.lookup(22).unwrap();
    if let SymbolKind::ClassReference { ref name, .. } = result.kind {
        assert_eq!(name, "Bar");
    } else {
        panic!("Expected ClassReference");
    }
}

// ── is_navigable_type tests ─────────────────────────────────────────

#[test]
fn scalar_types_are_not_navigable() {
    assert!(!is_navigable_type("int"));
    assert!(!is_navigable_type("string"));
    assert!(!is_navigable_type("bool"));
    assert!(!is_navigable_type("void"));
    assert!(!is_navigable_type("null"));
    assert!(!is_navigable_type("mixed"));
    assert!(!is_navigable_type("array"));
    assert!(!is_navigable_type("callable"));
    assert!(!is_navigable_type("float"));
    assert!(!is_navigable_type("never"));
    assert!(!is_navigable_type("iterable"));
    assert!(!is_navigable_type("true"));
    assert!(!is_navigable_type("false"));
    assert!(!is_navigable_type("resource"));
    assert!(!is_navigable_type("object"));
}

#[test]
fn class_names_are_navigable() {
    assert!(is_navigable_type("Foo"));
    assert!(is_navigable_type("Collection"));
    assert!(is_navigable_type("App\\Models\\User"));
    assert!(is_navigable_type("ResponseInterface"));
}

#[test]
fn case_insensitive_scalar_check() {
    assert!(!is_navigable_type("INT"));
    assert!(!is_navigable_type("String"));
    assert!(!is_navigable_type("BOOL"));
}

#[test]
fn empty_name_is_not_navigable() {
    assert!(!is_navigable_type(""));
}

// ── extract_symbol_map integration tests ────────────────────────────

fn parse_and_extract(php: &str) -> SymbolMap {
    let arena = bumpalo::Bump::new();
    let file_id = mago_database::file::FileId::new(b"test.php");
    let program = mago_syntax::parser::parse_file_content(&arena, file_id, php.as_bytes());
    extract_symbol_map(program, php)
}

#[test]
fn class_declaration_produces_class_declaration() {
    let php = "<?php\nclass Foo {}\n";
    let map = parse_and_extract(php);
    let hit = map.lookup(php.find("Foo").unwrap() as u32);
    assert!(hit.is_some());
    if let SymbolKind::ClassDeclaration { ref name } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassDeclaration, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn extends_produces_class_reference() {
    let php = "<?php\nclass Foo extends Bar {}\n";
    let map = parse_and_extract(php);
    let bar_offset = php.find("Bar").unwrap() as u32;
    let hit = map.lookup(bar_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Bar");
    } else {
        panic!("Expected ClassReference for Bar");
    }
}

#[test]
fn extends_fqn_sets_is_fqn() {
    let php = "<?php\nclass Foo extends \\App\\Bar {}\n";
    let map = parse_and_extract(php);
    // Find "\\App\\Bar" — the `\` at the start
    let fqn_offset = php.find("\\App\\Bar").unwrap() as u32;
    let hit = map.lookup(fqn_offset);
    assert!(hit.is_some(), "Should have a span at the FQN");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "App\\Bar");
        assert!(is_fqn, "FQN should be marked as is_fqn");
    } else {
        panic!(
            "Expected ClassReference for FQN, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn implements_produces_class_reference() {
    let php = "<?php\nclass Foo implements Baz, Qux {}\n";
    let map = parse_and_extract(php);

    let baz_offset = php.find("Baz").unwrap() as u32;
    let hit = map.lookup(baz_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Baz");
    } else {
        panic!("Expected ClassReference for Baz");
    }

    let qux_offset = php.find("Qux").unwrap() as u32;
    let hit = map.lookup(qux_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Qux");
    } else {
        panic!("Expected ClassReference for Qux");
    }
}

#[test]
fn variable_produces_variable_span() {
    let php = "<?php\nfunction test() { $foo = 1; }\n";
    let map = parse_and_extract(php);
    let offset = php.find("$foo").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::Variable { ref name } = hit.unwrap().kind {
        assert_eq!(name, "foo");
    } else {
        panic!("Expected Variable");
    }
}

#[test]
fn function_call_produces_function_call_span() {
    let php = "<?php\nfunction test() { strlen('hello'); }\n";
    let map = parse_and_extract(php);
    let offset = php.find("strlen").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::FunctionCall { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "strlen");
    } else {
        panic!("Expected FunctionCall, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn method_call_produces_member_access() {
    let php = "<?php\nclass Foo { function test() { $this->bar(); } }\n";
    let map = parse_and_extract(php);
    let bar_offset = php.find("bar").unwrap() as u32;
    let hit = map.lookup(bar_offset);
    assert!(hit.is_some());
    if let SymbolKind::MemberAccess {
        ref subject_text,
        ref member_name,
        is_static,
        is_method_call,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "bar");
        assert_eq!(subject_text, "$this");
        assert!(!is_static);
        assert!(is_method_call);
    } else {
        panic!("Expected MemberAccess");
    }
}

#[test]
fn static_method_call_produces_member_access() {
    let php = "<?php\nclass Foo { function test() { self::create(); } }\n";
    let map = parse_and_extract(php);
    let offset = php.find("create").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::MemberAccess {
        ref subject_text,
        ref member_name,
        is_static,
        is_method_call,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "create");
        assert_eq!(subject_text, "self");
        assert!(is_static);
        assert!(is_method_call);
    } else {
        panic!("Expected MemberAccess");
    }
}

#[test]
fn property_access_produces_member_access() {
    let php = "<?php\nclass Foo { function test() { $this->name; } }\n";
    let map = parse_and_extract(php);
    let arrow_pos = php.find("->name").unwrap();
    let name_offset = (arrow_pos + 2) as u32;
    let hit = map.lookup(name_offset);
    assert!(hit.is_some());
    if let SymbolKind::MemberAccess {
        ref member_name,
        is_method_call,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "name");
        assert!(!is_method_call);
    } else {
        panic!("Expected MemberAccess");
    }
}

#[test]
fn type_hint_produces_class_reference() {
    let php = "<?php\nfunction test(Foo $x): Bar { }\n";
    let map = parse_and_extract(php);

    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for Foo");
    }

    let bar_offset = php.find("Bar").unwrap() as u32;
    let hit = map.lookup(bar_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Bar");
    } else {
        panic!("Expected ClassReference for Bar");
    }
}

#[test]
fn scalar_type_hint_not_in_map() {
    let php = "<?php\nfunction test(int $x): string { }\n";
    let map = parse_and_extract(php);

    let int_offset = php.find("int").unwrap() as u32;
    assert!(map.lookup(int_offset).is_none());

    let string_offset = php.find("string").unwrap() as u32;
    assert!(map.lookup(string_offset).is_none());
}

#[test]
fn new_expression_produces_class_reference() {
    let php = "<?php\nfunction test() { $x = new Foo(); }\n";
    let map = parse_and_extract(php);
    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for Foo");
    }
}

#[test]
fn catch_type_produces_class_reference() {
    let php = "<?php\ntry {} catch (RuntimeException $e) {}\n";
    let map = parse_and_extract(php);
    let offset = php.find("RuntimeException").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "RuntimeException");
    } else {
        panic!("Expected ClassReference");
    }
}

#[test]
fn self_keyword_produces_self_static_parent() {
    let php = "<?php\nclass Foo { function test(): self { } }\n";
    let map = parse_and_extract(php);
    let offset = php.find("self").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::SelfStaticParent(kind) = &hit.unwrap().kind {
        assert_eq!(*kind, SelfStaticParentKind::Self_);
    } else {
        panic!("Expected SelfStaticParent");
    }
}

#[test]
fn whitespace_offset_returns_none() {
    let php = "<?php\nclass Foo    {}\n";
    let map = parse_and_extract(php);
    let foo_end = php.find("Foo").unwrap() + 3;
    let hit = map.lookup((foo_end + 1) as u32);
    assert!(hit.is_none());
}

#[test]
fn string_interior_not_navigable() {
    let php = "<?php\n$x = 'SomeClass';\n";
    let map = parse_and_extract(php);
    let some_offset = php.find("SomeClass").unwrap() as u32;
    let hit = map.lookup(some_offset);
    if let Some(span) = hit
        && let SymbolKind::ClassReference { .. } = &span.kind
    {
        panic!("Should not produce ClassReference inside a string literal");
    }
}

#[test]
fn chained_method_call_subject_text() {
    let php = "<?php\nclass Foo { function test() { $this->getService()->find(); } }\n";
    let map = parse_and_extract(php);
    let find_offset = php.find("find").unwrap() as u32;
    let hit = map.lookup(find_offset);
    assert!(hit.is_some());
    if let SymbolKind::MemberAccess {
        ref subject_text,
        ref member_name,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "find");
        assert_eq!(subject_text, "$this->getService()");
    } else {
        panic!("Expected MemberAccess");
    }
}

#[test]
fn class_constant_access_produces_member_access() {
    let php = "<?php\nclass Foo { function test() { self::MY_CONST; } }\n";
    let map = parse_and_extract(php);
    let offset = php.find("MY_CONST").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::MemberAccess {
        ref member_name,
        is_static,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "MY_CONST");
        assert!(is_static);
    } else {
        panic!("Expected MemberAccess");
    }
}

#[test]
fn trait_use_produces_class_reference() {
    let php = "<?php\nclass Foo { use SomeTrait; }\n";
    let map = parse_and_extract(php);
    let offset = php.find("SomeTrait").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "SomeTrait");
    } else {
        panic!("Expected ClassReference");
    }
}

#[test]
fn docblock_param_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @param UserService $service\n",
        "     * @return ResponseInterface\n",
        "     */\n",
        "    public function test($service) {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let user_service_offset = php.find("UserService").unwrap() as u32;
    let hit = map.lookup(user_service_offset);
    assert!(hit.is_some(), "Should find UserService in docblock");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "UserService");
    } else {
        panic!("Expected ClassReference for UserService");
    }

    let response_offset = php.find("ResponseInterface").unwrap() as u32;
    let hit = map.lookup(response_offset);
    assert!(hit.is_some(), "Should find ResponseInterface in docblock");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "ResponseInterface");
    } else {
        panic!("Expected ClassReference for ResponseInterface");
    }
}

#[test]
fn docblock_scalar_param_not_navigable() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name\n",
        " */\n",
        "function test($name) {}\n",
    );
    let map = parse_and_extract(php);
    let string_offset = php.find("string").unwrap() as u32;
    let hit = map.lookup(string_offset);
    if let Some(span) = hit
        && let SymbolKind::ClassReference { .. } = &span.kind
    {
        panic!("Scalar type 'string' should not produce a ClassReference");
    }
}

#[test]
fn nullable_type_hint_produces_class_reference() {
    let php = "<?php\nfunction test(?Foo $x) {}\n";
    let map = parse_and_extract(php);
    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for nullable Foo");
    }
}

#[test]
fn interface_declaration_produces_declaration() {
    let php = "<?php\ninterface Serializable {}\n";
    let map = parse_and_extract(php);
    let offset = php.find("Serializable").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassDeclaration { ref name } = hit.unwrap().kind {
        assert_eq!(name, "Serializable");
    } else {
        panic!("Expected ClassDeclaration, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn enum_declaration_produces_declaration() {
    let php = "<?php\nenum Color { case Red; case Blue; }\n";
    let map = parse_and_extract(php);
    let offset = php.find("Color").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassDeclaration { ref name } = hit.unwrap().kind {
        assert_eq!(name, "Color");
    } else {
        panic!("Expected ClassDeclaration, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn enum_case_produces_member_declaration() {
    let php = "<?php\nenum Color { case Red; case Blue; }\n";
    let map = parse_and_extract(php);

    // Unit enum case `Red`
    let red_offset = php.find("Red").unwrap() as u32;
    let hit = map.lookup(red_offset);
    assert!(hit.is_some(), "Expected a symbol span for enum case Red");
    if let SymbolKind::MemberDeclaration {
        ref name,
        is_static,
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Red");
        assert!(is_static, "Enum cases are accessed statically");
    } else {
        panic!(
            "Expected MemberDeclaration for enum case Red, got {:?}",
            hit.unwrap().kind
        );
    }

    // Unit enum case `Blue`
    let blue_offset = php.find("Blue").unwrap() as u32;
    let hit = map.lookup(blue_offset);
    assert!(hit.is_some(), "Expected a symbol span for enum case Blue");
    if let SymbolKind::MemberDeclaration {
        ref name,
        is_static,
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Blue");
        assert!(is_static, "Enum cases are accessed statically");
    } else {
        panic!(
            "Expected MemberDeclaration for enum case Blue, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn backed_enum_case_produces_member_declaration() {
    let php = "<?php\nenum TaskType: int { case Task = 1; case Issue = 2; }\n";
    let map = parse_and_extract(php);

    let issue_offset = php.find("Issue").unwrap() as u32;
    let hit = map.lookup(issue_offset);
    assert!(
        hit.is_some(),
        "Expected a symbol span for backed enum case Issue"
    );
    if let SymbolKind::MemberDeclaration {
        ref name,
        is_static,
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Issue");
        assert!(is_static, "Enum cases are accessed statically");
    } else {
        panic!(
            "Expected MemberDeclaration for backed enum case Issue, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn closure_param_type_hint() {
    let php = "<?php\n$f = function(Foo $x): Bar {};\n";
    let map = parse_and_extract(php);

    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for Foo");
    }
}

#[test]
fn instanceof_rhs_produces_class_reference() {
    let php = "<?php\nfunction test($x) { if ($x instanceof Foo) {} }\n";
    let map = parse_and_extract(php);
    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some());
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for instanceof Foo");
    }
}

#[test]
fn docblock_union_type_produces_multiple_references() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @return Foo|Bar\n",
        " */\n",
        "function test() {}\n",
    );
    let map = parse_and_extract(php);

    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some(), "Should find Foo in union return type");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    }

    let bar_offset = php.find("Bar").unwrap() as u32;
    let hit = map.lookup(bar_offset);
    assert!(hit.is_some(), "Should find Bar in union return type");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Bar");
    }
}

#[test]
fn docblock_nullable_type() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @return ?Foo\n",
        " */\n",
        "function test() {}\n",
    );
    let map = parse_and_extract(php);
    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some(), "Should find Foo in nullable return type");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    }
}

#[test]
fn docblock_phpstan_require_extends_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "use Illuminate\\Http\\Resources\\Json\\JsonResource;\n",
        "/** @phpstan-require-extends JsonResource */\n",
        "trait GathersAuthorizations {}\n"
    );
    let map = parse_and_extract(php);
    let docblock_start = php.find("/**").unwrap();
    let name_in_doc = php[docblock_start..].find("JsonResource").unwrap() + docblock_start;

    let hit = map.lookup(name_in_doc as u32);
    assert!(
        hit.is_some(),
        "Should find JsonResource in require-extends docblock"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "JsonResource");
    } else {
        panic!("Expected ClassReference for JsonResource");
    }
}

#[test]
fn docblock_phpstan_require_implements_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "use Countable;\n",
        "/** @phpstan-require-implements Countable */\n",
        "trait GathersAuthorizations {}\n"
    );
    let map = parse_and_extract(php);
    let docblock_start = php.find("/**").unwrap();
    let name_in_doc = php[docblock_start..].find("Countable").unwrap() + docblock_start;

    let hit = map.lookup(name_in_doc as u32);
    assert!(
        hit.is_some(),
        "Should find Countable in require-implements docblock"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Countable");
    } else {
        panic!("Expected ClassReference for Countable");
    }
}

#[test]
fn docblock_phpstan_sealed_produces_class_references() {
    let php = concat!(
        "<?php\n",
        "use App\\FooClass;\n",
        "use App\\BarClass;\n",
        "/** @phpstan-sealed FooClass|BarClass */\n",
        "class BaseClass {}\n"
    );
    let map = parse_and_extract(php);
    let docblock_start = php.find("/**").unwrap();
    let foo_in_doc = php[docblock_start..].find("FooClass").unwrap() + docblock_start;

    let hit = map.lookup(foo_in_doc as u32);
    assert!(
        hit.is_some(),
        "Should find FooClass in @phpstan-sealed docblock"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "FooClass");
    } else {
        panic!("Expected ClassReference for FooClass");
    }

    let bar_in_doc = php[docblock_start..].find("BarClass").unwrap() + docblock_start;
    let hit2 = map.lookup(bar_in_doc as u32);
    assert!(
        hit2.is_some(),
        "Should find BarClass in @phpstan-sealed docblock"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit2.unwrap().kind {
        assert_eq!(name, "BarClass");
    } else {
        panic!("Expected ClassReference for BarClass");
    }
}

#[test]
fn docblock_fqn_type() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @return \\App\\Models\\User\n",
        " */\n",
        "function test() {}\n",
    );
    let map = parse_and_extract(php);
    let user_offset = php.find("\\App\\Models\\User").unwrap() as u32;
    let hit = map.lookup(user_offset);
    assert!(hit.is_some(), "Should find FQN type in docblock");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "App\\Models\\User");
        assert!(is_fqn, "Docblock FQN type should have is_fqn = true");
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn docblock_this_produces_self_static_parent() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @return Collection<Item, $this>\n",
        "     */\n",
        "    public function items() {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);
    let this_offset = php.find("$this").unwrap() as u32;
    let hit = map.lookup(this_offset);
    assert!(hit.is_some(), "Should find $this in docblock generic arg");
    if let SymbolKind::SelfStaticParent(kind) = &hit.unwrap().kind {
        assert_eq!(*kind, SelfStaticParentKind::This);
    } else {
        panic!(
            "Expected SelfStaticParent for $this, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn attribute_class_reference() {
    let php = concat!(
        "<?php\n",
        "#[\\Illuminate\\Database\\Eloquent\\Attributes\\CollectedBy(ReviewCollection::class)]\n",
        "class Review {}\n",
    );
    let map = parse_and_extract(php);

    // The attribute class name should be a ClassReference.
    let attr_offset = php
        .find("\\Illuminate\\Database\\Eloquent\\Attributes\\CollectedBy")
        .unwrap() as u32;
    let hit = map.lookup(attr_offset);
    assert!(hit.is_some(), "Should find attribute class reference");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(
            name,
            "Illuminate\\Database\\Eloquent\\Attributes\\CollectedBy"
        );
        assert!(is_fqn, "Attribute FQN should have is_fqn = true");
    } else {
        panic!(
            "Expected ClassReference for attribute, got {:?}",
            hit.unwrap().kind
        );
    }

    // The argument `ReviewCollection::class` should produce a ClassReference for ReviewCollection.
    let rc_offset = php.find("ReviewCollection").unwrap() as u32;
    let hit = map.lookup(rc_offset);
    assert!(
        hit.is_some(),
        "Should find ReviewCollection in attribute argument"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "ReviewCollection");
    } else {
        panic!(
            "Expected ClassReference for ReviewCollection, got {:?}",
            hit.unwrap().kind
        );
    }

    // The class declaration name `Review` should be ClassDeclaration, not ClassReference.
    let review_offset = php.find("class Review").unwrap() as u32 + 6; // skip "class "
    let hit = map.lookup(review_offset);
    assert!(hit.is_some(), "Should find Review declaration");
    if let SymbolKind::ClassDeclaration { ref name } = hit.unwrap().kind {
        assert_eq!(name, "Review");
    } else {
        panic!(
            "Expected ClassDeclaration for Review, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn fqn_type_hint_in_parameter() {
    let php = "<?php\nfunction test(\\Illuminate\\Support\\Collection $c) {}\n";
    let map = parse_and_extract(php);
    let fqn_offset = php.find("\\Illuminate\\Support\\Collection").unwrap() as u32;
    let hit = map.lookup(fqn_offset);
    assert!(hit.is_some(), "Should find FQN type hint in parameter");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Illuminate\\Support\\Collection");
        assert!(is_fqn, "FQN parameter type hint should have is_fqn = true");
    } else {
        panic!(
            "Expected ClassReference for FQN param hint, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn fqn_extends_class_reference() {
    let php = "<?php\nclass Review extends \\Illuminate\\Database\\Eloquent\\Model {}\n";
    let map = parse_and_extract(php);
    let fqn_offset = php.find("\\Illuminate\\Database\\Eloquent\\Model").unwrap() as u32;
    let hit = map.lookup(fqn_offset);
    assert!(hit.is_some(), "Should find FQN in extends clause");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Illuminate\\Database\\Eloquent\\Model");
        assert!(is_fqn, "FQN extends should have is_fqn = true");
    } else {
        panic!(
            "Expected ClassReference for FQN extends, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn fqn_lookup_at_middle_of_name() {
    // Verify that the symbol span covers the ENTIRE FQN so that
    // clicking anywhere within it (not just at the leading `\`)
    // resolves correctly.
    let php = concat!(
        "<?php\n",
        "#[\\Illuminate\\Database\\Eloquent\\Attributes\\CollectedBy(ReviewCollection::class)]\n",
        "class Review extends \\Illuminate\\Database\\Eloquent\\Model\n",
        "{\n",
        "    /** @return \\Illuminate\\Database\\Eloquent\\Relations\\HasMany<Review, $this> */\n",
        "    public function replies(): mixed { return $this->hasMany(Review::class); }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // ── Attribute FQN: click on "CollectedBy" (last segment) ──
    let cb_offset = php.find("CollectedBy").unwrap() as u32;
    let hit = map.lookup(cb_offset);
    assert!(
        hit.is_some(),
        "Should find attribute FQN when cursor is on 'CollectedBy'"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(
            name,
            "Illuminate\\Database\\Eloquent\\Attributes\\CollectedBy"
        );
        assert!(is_fqn);
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }

    // ── Attribute FQN: click on "Database" (middle segment) ──
    // Find the first "Database" which is inside the attribute
    let db_attr_offset = php.find("Database").unwrap() as u32;
    let hit = map.lookup(db_attr_offset);
    assert!(
        hit.is_some(),
        "Should find attribute FQN when cursor is on 'Database'"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(
            name,
            "Illuminate\\Database\\Eloquent\\Attributes\\CollectedBy"
        );
        assert!(is_fqn);
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }

    // ── Extends FQN: click on "Model" (last segment) ──
    let model_offset = php.find("Model\n").unwrap() as u32;
    let hit = map.lookup(model_offset);
    assert!(
        hit.is_some(),
        "Should find extends FQN when cursor is on 'Model'"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Illuminate\\Database\\Eloquent\\Model");
        assert!(is_fqn);
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }

    // ── Extends FQN: click on "Eloquent" (middle segment) ──
    // The second "Eloquent" is in the extends clause
    let extends_line_start = php.find("class Review extends").unwrap();
    let eloquent_in_extends =
        php[extends_line_start..].find("Eloquent").unwrap() + extends_line_start;
    let hit = map.lookup(eloquent_in_extends as u32);
    assert!(
        hit.is_some(),
        "Should find extends FQN when cursor is on 'Eloquent'"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Illuminate\\Database\\Eloquent\\Model");
        assert!(is_fqn);
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }

    // ── Docblock FQN: click on "HasMany" (last segment) ──
    let hm_offset = php.find("HasMany").unwrap() as u32;
    let hit = map.lookup(hm_offset);
    assert!(
        hit.is_some(),
        "Should find docblock FQN when cursor is on 'HasMany'"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Illuminate\\Database\\Eloquent\\Relations\\HasMany");
        assert!(is_fqn);
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }

    // ── Docblock FQN: click on "Relations" (middle segment) ──
    let rel_offset = php.find("Relations").unwrap() as u32;
    let hit = map.lookup(rel_offset);
    assert!(
        hit.is_some(),
        "Should find docblock FQN when cursor is on 'Relations'"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Illuminate\\Database\\Eloquent\\Relations\\HasMany");
        assert!(is_fqn);
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }

    // ── Docblock $this inside generic args ──
    let docblock_start = php.find("/** @return").unwrap();
    let this_in_doc = php[docblock_start..].find("$this").unwrap() + docblock_start;
    let hit = map.lookup(this_in_doc as u32);
    assert!(hit.is_some(), "Should find $this in docblock generic arg");
    if let SymbolKind::SelfStaticParent(kind) = &hit.unwrap().kind {
        assert_eq!(*kind, SelfStaticParentKind::This);
    } else {
        panic!(
            "Expected SelfStaticParent for $this, got {:?}",
            hit.unwrap().kind
        );
    }
}

// ── @template tag tests ─────────────────────────────────────────────

#[test]
fn template_tag_bound_type_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template-covariant TNode of AstNode\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);
    let ast_offset = php.find("AstNode").unwrap() as u32;
    let hit = map.lookup(ast_offset);
    assert!(
        hit.is_some(),
        "Should find bound type AstNode in @template tag"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "AstNode");
        assert!(!is_fqn);
    } else {
        panic!(
            "Expected ClassReference for AstNode, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn template_tag_without_bound_produces_no_span() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);
    // "T" should NOT produce a ClassReference — it's a parameter name.
    let t_offset = php.find(" T\n").unwrap() as u32 + 1; // offset of 'T'
    let hit = map.lookup(t_offset);
    assert!(
        hit.is_none(),
        "Template parameter name should not be navigable"
    );
}

#[test]
fn template_tag_fqn_bound() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template T of \\App\\Contracts\\Renderable\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);
    let r_offset = php.find("\\App\\Contracts\\Renderable").unwrap() as u32;
    let hit = map.lookup(r_offset);
    assert!(hit.is_some(), "Should find FQN bound type");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "App\\Contracts\\Renderable");
        assert!(is_fqn);
    } else {
        panic!("Expected ClassReference, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn template_covariant_and_contravariant_tags() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template-covariant TOut of Output\n",
        " * @template-contravariant TIn of Input\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let out_offset = php.find("Output").unwrap() as u32;
    let hit = map.lookup(out_offset);
    assert!(hit.is_some(), "Should find Output bound");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Output");
    } else {
        panic!("Expected ClassReference for Output");
    }

    let in_offset = php.find("Input").unwrap() as u32;
    let hit = map.lookup(in_offset);
    assert!(hit.is_some(), "Should find Input bound");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Input");
    } else {
        panic!("Expected ClassReference for Input");
    }
}

#[test]
fn phpstan_template_tag() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @phpstan-template T of Collection\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);
    let c_offset = php.find("Collection").unwrap() as u32;
    let hit = map.lookup(c_offset);
    assert!(
        hit.is_some(),
        "Should find Collection bound from @phpstan-template"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Collection");
    } else {
        panic!("Expected ClassReference for Collection");
    }
}

// ── VarDefSite extraction tests ─────────────────────────────────────

#[test]
fn var_def_assignment_in_function() {
    let php = "<?php\nfunction foo() {\n    $x = 42;\n}\n";
    let map = parse_and_extract(php);
    assert!(
        !map.var_defs.is_empty(),
        "Should have at least one VarDefSite"
    );
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "x")
        .expect("Should find $x def");
    assert_eq!(def.kind, VarDefKind::Assignment);
    // The definition should be inside the function scope.
    assert_ne!(
        def.scope_start, 0,
        "scope_start should be function body brace, not top-level"
    );
}

#[test]
fn var_def_parameter_in_function() {
    let php = "<?php\nfunction greet(string $name) {\n    echo $name;\n}\n";
    let map = parse_and_extract(php);
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "name" && d.kind == VarDefKind::Parameter);
    assert!(def.is_some(), "Should find parameter $name as VarDefSite");
}

#[test]
fn var_def_foreach_key_and_value() {
    let php = "<?php\nfunction f() {\n    foreach ($items as $key => $val) { }\n}\n";
    let map = parse_and_extract(php);
    let key_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "key" && d.kind == VarDefKind::Foreach);
    let val_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "val" && d.kind == VarDefKind::Foreach);
    assert!(key_def.is_some(), "Should find foreach key $key");
    assert!(val_def.is_some(), "Should find foreach value $val");
}

#[test]
fn var_def_catch_variable() {
    let php = "<?php\nfunction f() {\n    try { } catch (Exception $e) { }\n}\n";
    let map = parse_and_extract(php);
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "e" && d.kind == VarDefKind::Catch);
    assert!(def.is_some(), "Should find catch variable $e");
}

#[test]
fn var_def_static_variable() {
    let php = "<?php\nfunction f() {\n    static $counter = 0;\n}\n";
    let map = parse_and_extract(php);
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "counter" && d.kind == VarDefKind::StaticDecl);
    assert!(def.is_some(), "Should find static variable $counter");
}

#[test]
fn var_def_global_variable() {
    let php = "<?php\nfunction f() {\n    global $db;\n}\n";
    let map = parse_and_extract(php);
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "db" && d.kind == VarDefKind::GlobalDecl);
    assert!(def.is_some(), "Should find global variable $db");
}

#[test]
fn var_def_array_destructuring() {
    let php = "<?php\nfunction f() {\n    [$a, $b] = explode(',', $str);\n}\n";
    let map = parse_and_extract(php);
    let a_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "a" && d.kind == VarDefKind::ArrayDestructuring);
    let b_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "b" && d.kind == VarDefKind::ArrayDestructuring);
    assert!(a_def.is_some(), "Should find $a from array destructuring");
    assert!(b_def.is_some(), "Should find $b from array destructuring");
}

#[test]
fn var_def_list_destructuring() {
    let php = "<?php\nfunction f() {\n    list($a, $b) = func();\n}\n";
    let map = parse_and_extract(php);
    let a_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "a" && d.kind == VarDefKind::ListDestructuring);
    let b_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "b" && d.kind == VarDefKind::ListDestructuring);
    assert!(a_def.is_some(), "Should find $a from list destructuring");
    assert!(b_def.is_some(), "Should find $b from list destructuring");
}

#[test]
fn var_def_method_parameter() {
    let php =
        "<?php\nclass Foo {\n    public function bar(int $x) {\n        return $x;\n    }\n}\n";
    let map = parse_and_extract(php);
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "x" && d.kind == VarDefKind::Parameter);
    assert!(def.is_some(), "Should find method parameter $x");
}

#[test]
fn var_def_closure_parameter() {
    let php = "<?php\nfunction f() {\n    $fn = function (string $s) { return $s; };\n}\n";
    let map = parse_and_extract(php);
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "s" && d.kind == VarDefKind::Parameter);
    assert!(def.is_some(), "Should find closure parameter $s");
}

#[test]
fn var_def_arrow_function_parameter() {
    let php = "<?php\nfunction f() {\n    $fn = fn(int $n) => $n * 2;\n}\n";
    let map = parse_and_extract(php);
    let def = map
        .var_defs
        .iter()
        .find(|d| d.name == "n" && d.kind == VarDefKind::Parameter);
    assert!(def.is_some(), "Should find arrow function parameter $n");
}

// ── Scope tracking tests ────────────────────────────────────────────

#[test]
fn scopes_populated_for_function() {
    let php = "<?php\nfunction foo() {\n    $x = 1;\n}\n";
    let map = parse_and_extract(php);
    assert!(
        !map.scopes.is_empty(),
        "Should have at least one scope for the function body"
    );
}

#[test]
fn scopes_populated_for_method() {
    let php = "<?php\nclass A {\n    public function m() {\n        $y = 2;\n    }\n}\n";
    let map = parse_and_extract(php);
    assert!(
        !map.scopes.is_empty(),
        "Should have at least one scope for the method body"
    );
}

#[test]
fn scopes_populated_for_closure() {
    let php = "<?php\nfunction f() {\n    $fn = function () { $z = 3; };\n}\n";
    let map = parse_and_extract(php);
    // One for the outer function, one for the closure.
    assert!(
        map.scopes.len() >= 2,
        "Should have scopes for both function and closure"
    );
}

#[test]
fn find_enclosing_scope_top_level() {
    let php = "<?php\n$x = 1;\n";
    let map = parse_and_extract(php);
    // Top-level offset should return scope_start 0.
    assert_eq!(map.find_enclosing_scope(7), 0);
}

#[test]
fn find_enclosing_scope_inside_function() {
    let php = "<?php\nfunction foo() {\n    $x = 1;\n}\n";
    let map = parse_and_extract(php);
    // Offset inside the function body should return the function's scope_start.
    let body_offset = php.find('{').unwrap() as u32;
    let x_offset = php.find("$x").unwrap() as u32;
    let scope = map.find_enclosing_scope(x_offset);
    assert_eq!(
        scope, body_offset,
        "Should find the function body as the enclosing scope"
    );
}

// ── find_var_definition tests ───────────────────────────────────────

#[test]
fn find_var_definition_returns_most_recent() {
    let php = "<?php\nfunction f() {\n    $x = 1;\n    $x = 2;\n    echo $x;\n}\n";
    let map = parse_and_extract(php);
    let echo_x_offset = php.rfind("$x").unwrap() as u32;
    let scope = map.find_enclosing_scope(echo_x_offset);
    let def = map.find_var_definition("x", echo_x_offset, scope);
    assert!(def.is_some(), "Should find a definition for $x");
    // The most recent definition should be `$x = 2;` not `$x = 1;`
    let second_assign_offset = php.find("$x = 2").unwrap() as u32;
    assert_eq!(
        def.unwrap().offset,
        second_assign_offset,
        "Should find the second assignment"
    );
}

#[test]
fn find_var_definition_parameter_found() {
    let php = "<?php\nfunction greet(string $name) {\n    echo $name;\n}\n";
    let map = parse_and_extract(php);
    let echo_name_offset = php.rfind("$name").unwrap() as u32;
    let scope = map.find_enclosing_scope(echo_name_offset);
    let def = map.find_var_definition("name", echo_name_offset, scope);
    assert!(def.is_some(), "Should find parameter $name");
    assert_eq!(def.unwrap().kind, VarDefKind::Parameter);
}

#[test]
fn find_var_definition_none_when_no_def() {
    let php = "<?php\nfunction f() {\n    echo $undefined;\n}\n";
    let map = parse_and_extract(php);
    let offset = php.find("$undefined").unwrap() as u32;
    let scope = map.find_enclosing_scope(offset);
    let def = map.find_var_definition("undefined", offset, scope);
    assert!(def.is_none(), "Should return None for undefined variable");
}

#[test]
fn find_var_definition_respects_scope() {
    let php = concat!(
        "<?php\n",
        "function outer() {\n",
        "    $x = 'outer';\n",
        "    $fn = function () {\n",
        "        echo $x;\n", // $x not defined in closure scope
        "    };\n",
        "}\n",
    );
    let map = parse_and_extract(php);
    let echo_x_offset = php.rfind("$x").unwrap() as u32;
    let scope = map.find_enclosing_scope(echo_x_offset);
    let def = map.find_var_definition("x", echo_x_offset, scope);
    // $x is defined in outer scope, not closure scope, so should be None.
    assert!(def.is_none(), "Should not find $x from a different scope");
}

#[test]
fn assignment_effective_from_excludes_rhs() {
    // In `$x = $x + 1;`, the RHS $x should see the *previous* definition,
    // not the one being written.
    let php = concat!(
        "<?php\n",
        "function f() {\n",
        "    $x = 10;\n",
        "    $x = $x + 1;\n",
        "}\n",
    );
    let map = parse_and_extract(php);
    // The RHS `$x` in `$x = $x + 1;`
    let rhs_x_offset = php.rfind("$x + 1").unwrap() as u32;
    let scope = map.find_enclosing_scope(rhs_x_offset);
    let def = map.find_var_definition("x", rhs_x_offset, scope);
    assert!(def.is_some(), "Should find a definition for RHS $x");
    // Should point to `$x = 10;`, not `$x = $x + 1;`
    let first_assign_offset = php.find("$x = 10").unwrap() as u32;
    assert_eq!(
        def.unwrap().offset,
        first_assign_offset,
        "RHS $x should see the first assignment, not the one being written"
    );
}

#[test]
fn assignment_effective_from_excludes_rhs_in_constructor_args() {
    // In `$request = new Foo(arg: $request->uuid)`, the `$request`
    // inside the constructor arguments should see the *previous* definition
    // (the parameter), not the assignment being written.  PHP evaluates
    // all RHS arguments before performing the assignment.
    let php = concat!(
        "<?php\n",
        "function f(Foo $request) {\n",
        "    $request = new Bar(\n",
        "        name: $request->uuid,\n",
        "    );\n",
        "}\n",
    );
    let map = parse_and_extract(php);
    // The `$request` in `$request->uuid` (inside the constructor args)
    let rhs_request_offset = php.find("$request->uuid").unwrap() as u32;
    let scope = map.find_enclosing_scope(rhs_request_offset);
    let def = map.find_var_definition("request", rhs_request_offset, scope);
    assert!(def.is_some(), "Should find a definition for RHS $request");
    // Should point to the parameter `$request`, not the assignment LHS.
    let param_offset = php.find("$request)").unwrap() as u32;
    assert_eq!(
        def.unwrap().offset,
        param_offset,
        "RHS $request inside constructor args should see the parameter, not the assignment"
    );
    assert_eq!(def.unwrap().kind, VarDefKind::Parameter);
}

// ── is_at_var_definition tests ──────────────────────────────────────

#[test]
fn is_at_var_definition_on_assignment_lhs() {
    let php = "<?php\nfunction f() {\n    $x = 42;\n}\n";
    let map = parse_and_extract(php);
    let x_offset = php.find("$x = 42").unwrap() as u32;
    assert!(
        map.is_at_var_definition("x", x_offset),
        "Should detect cursor on assignment LHS as at-definition"
    );
    // One byte into the token (on the 'x')
    assert!(
        map.is_at_var_definition("x", x_offset + 1),
        "Should detect cursor on 'x' of '$x' as at-definition"
    );
}

#[test]
fn is_at_var_definition_on_parameter() {
    let php = "<?php\nfunction greet(string $name) {\n    echo $name;\n}\n";
    let map = parse_and_extract(php);
    let param_offset = php.find("$name)").unwrap() as u32;
    assert!(
        map.is_at_var_definition("name", param_offset),
        "Should detect cursor on parameter as at-definition"
    );
}

#[test]
fn is_at_var_definition_false_on_usage() {
    let php = "<?php\nfunction f() {\n    $x = 42;\n    echo $x;\n}\n";
    let map = parse_and_extract(php);
    let echo_x_offset = php.rfind("$x").unwrap() as u32;
    assert!(
        !map.is_at_var_definition("x", echo_x_offset),
        "Should NOT detect cursor on variable usage as at-definition"
    );
}

#[test]
fn nested_array_destructuring_var_defs() {
    let php = "<?php\nfunction f() {\n    [[$a, $b], $c] = getData();\n}\n";
    let map = parse_and_extract(php);
    let a_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "a" && d.kind == VarDefKind::ArrayDestructuring);
    let b_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "b" && d.kind == VarDefKind::ArrayDestructuring);
    let c_def = map
        .var_defs
        .iter()
        .find(|d| d.name == "c" && d.kind == VarDefKind::ArrayDestructuring);
    assert!(a_def.is_some(), "Should find $a from nested destructuring");
    assert!(b_def.is_some(), "Should find $b from nested destructuring");
    assert!(c_def.is_some(), "Should find $c from outer destructuring");
}

#[test]
fn var_defs_sorted_by_scope_start_then_offset() {
    let php = concat!(
        "<?php\n",
        "function a() {\n",
        "    $x = 1;\n",
        "    $y = 2;\n",
        "}\n",
        "function b() {\n",
        "    $z = 3;\n",
        "}\n",
    );
    let map = parse_and_extract(php);
    // Verify the var_defs are sorted by (scope_start, offset).
    for window in map.var_defs.windows(2) {
        let (a, b) = (&window[0], &window[1]);
        assert!(
            (a.scope_start, a.offset) <= (b.scope_start, b.offset),
            "var_defs should be sorted by (scope_start, offset): ({}, {}) vs ({}, {})",
            a.scope_start,
            a.offset,
            b.scope_start,
            b.offset,
        );
    }
}

#[test]
fn top_level_var_def_has_scope_start_zero() {
    let php = "<?php\n$global = 'hello';\n";
    let map = parse_and_extract(php);
    let def = map.var_defs.iter().find(|d| d.name == "global");
    assert!(def.is_some(), "Should find top-level $global");
    assert_eq!(
        def.unwrap().scope_start,
        0,
        "Top-level definitions should have scope_start 0"
    );
}

// ── Template param definition lookup tests ──────────────────────────

#[test]
fn template_param_def_recorded_for_class() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TModel\n",
        " */\n",
        "class Collection {\n",
        "    /** @return array<TKey, TModel> */\n",
        "    public function all(): array { return []; }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    assert!(
        map.template_defs.len() >= 2,
        "Should record at least 2 template defs, got {}",
        map.template_defs.len()
    );

    let tkey = map.template_defs.iter().find(|d| d.name == "TKey");
    assert!(tkey.is_some(), "Should find TKey template def");
    let tkey = tkey.unwrap();
    assert_eq!(
        &php[tkey.name_offset as usize..(tkey.name_offset + 4) as usize],
        "TKey",
        "name_offset should point to the TKey text"
    );
    assert_eq!(
        tkey.bound,
        Some(crate::php_type::PhpType::parse("array-key")),
        "TKey should have bound 'array-key'"
    );

    let tmodel = map.template_defs.iter().find(|d| d.name == "TModel");
    assert!(tmodel.is_some(), "Should find TModel template def");
    assert_eq!(tmodel.unwrap().bound, None, "TModel should have no bound");
}

#[test]
fn template_param_def_lookup_in_same_class() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TModel\n",
        " */\n",
        "class Collection {\n",
        "    /** @return array<TKey, TModel> */\n",
        "    public function all(): array { return []; }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // Cursor inside the class body (on the @return line) should find TKey
    let return_line_offset = php.find("@return").unwrap() as u32;
    let found = map.find_template_def("TKey", return_line_offset);
    assert!(
        found.is_some(),
        "Should find TKey from within the class body"
    );
    assert_eq!(found.unwrap().name, "TKey");

    let found = map.find_template_def("TModel", return_line_offset);
    assert!(
        found.is_some(),
        "Should find TModel from within the class body"
    );
}

#[test]
fn template_param_def_not_found_outside_scope() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Foo {}\n",
        "class Bar {}\n",
    );
    let map = parse_and_extract(php);

    let bar_offset = php.find("class Bar").unwrap() as u32;
    let found = map.find_template_def("T", bar_offset);
    assert!(found.is_none(), "T should NOT be found outside Foo's scope");
}

#[test]
fn template_param_def_method_level() {
    let php = concat!(
        "<?php\n",
        "class Mapper {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $item\n",
        "     * @return T\n",
        "     */\n",
        "    public function wrap(object $item): object { return $item; }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let t_def = map.template_defs.iter().find(|d| d.name == "T");
    assert!(t_def.is_some(), "Should find method-level template T");

    // Should be findable from within the method's docblock
    let param_line = php.find("@param T").unwrap() as u32;
    let found = map.find_template_def("T", param_line);
    assert!(
        found.is_some(),
        "Should find T from within the method docblock"
    );
}

#[test]
fn callable_param_type_spans_are_tight() {
    // Verify that TKey inside `callable(TValue, TKey): mixed` gets a
    // ClassReference span that covers exactly `TKey`, not `TKey): mixed …`.
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template TKey of array-key\n",
        " * @template TValue\n",
        " */\n",
        "class Col {\n",
        "    /**\n",
        "     * @param callable(TValue, TKey): mixed $callback\n",
        "     * @return static\n",
        "     */\n",
        "    public function each(callable $callback): static { return $this; }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // Find the byte offset of `TKey` inside the @param callable line.
    let param_line = php.find("@param callable(TValue, TKey): mixed").unwrap();
    let tkey_offset = php[param_line..].find("TKey").unwrap() + param_line;

    // There should be a ClassReference span starting exactly at tkey_offset.
    let span = map
        .spans
        .iter()
        .find(|s| s.start == tkey_offset as u32)
        .unwrap_or_else(|| {
            panic!(
                "Should find a span starting at TKey offset {}; spans: {:?}",
                tkey_offset,
                map.spans
                    .iter()
                    .filter(|s| {
                        matches!(
                            &s.kind,
                            SymbolKind::ClassReference { name, .. } if name == "TKey" || name == "TValue"
                        )
                    })
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        span.end - span.start,
        4,
        "TKey span should be exactly 4 bytes wide, but was {} ({}..{}), covering {:?}",
        span.end - span.start,
        span.start,
        span.end,
        &php[span.start as usize..span.end as usize],
    );

    assert!(
        matches!(&span.kind, SymbolKind::ClassReference { name, .. } if name == "TKey"),
        "span at TKey offset should be a ClassReference for TKey, got {:?}",
        span.kind,
    );
}

// ── $this as SelfStaticParent tests ─────────────────────────────────

#[test]
fn this_variable_emits_self_static_parent() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): void {\n",
        "        $this->baz();\n",
        "    }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // Find the $this token (not the one inside ->baz subject_text)
    let this_offset = php.find("$this->baz").unwrap() as u32;
    let hit = map.lookup(this_offset);
    assert!(hit.is_some(), "Should find a span at $this");
    match &hit.unwrap().kind {
        SymbolKind::SelfStaticParent(kind) => {
            assert_eq!(
                *kind,
                SelfStaticParentKind::This,
                "$this should map to This variant"
            );
        }
        other => panic!("Expected SelfStaticParent for $this, got {:?}", other),
    }
}

#[test]
fn this_variable_standalone_emits_self_static_parent() {
    // `$this` on its own (not as part of ->)
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function bar(): self {\n",
        "        return $this;\n",
        "    }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let this_offset = php.find("$this;").unwrap() as u32;
    let hit = map.lookup(this_offset);
    assert!(hit.is_some(), "Should find a span at standalone $this");
    match &hit.unwrap().kind {
        SymbolKind::SelfStaticParent(kind) => {
            assert_eq!(*kind, SelfStaticParentKind::This);
        }
        other => panic!(
            "Expected SelfStaticParent for standalone $this, got {:?}",
            other
        ),
    }
}

#[test]
fn regular_variable_still_emits_variable() {
    let php = "<?php\nfunction f() { $x = 1; }\n";
    let map = parse_and_extract(php);

    let x_offset = php.find("$x").unwrap() as u32;
    let hit = map.lookup(x_offset);
    assert!(hit.is_some());
    match &hit.unwrap().kind {
        SymbolKind::Variable { name } => {
            assert_eq!(name, "x", "$x should still emit Variable");
        }
        other => panic!("Expected Variable for $x, got {:?}", other),
    }
}

// ── Array suffix stripping tests ────────────────────────────────────

#[test]
fn docblock_array_suffix_type_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "class AstNode {}\n",
        "class Foo {\n",
        "    /** @return AstNode[] */\n",
        "    public function getChildren(): array { return []; }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // Find the AstNode in the @return tag (not the class declaration)
    let docblock_start = php.find("/** @return").unwrap();
    let ast_in_doc = php[docblock_start..].find("AstNode").unwrap() + docblock_start;
    let hit = map.lookup(ast_in_doc as u32);
    assert!(hit.is_some(), "Should find AstNode in @return AstNode[]");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(
            name, "AstNode",
            "Name should be 'AstNode' without [] suffix"
        );
    } else {
        panic!(
            "Expected ClassReference for AstNode[], got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn docblock_array_suffix_span_excludes_brackets() {
    let php = concat!(
        "<?php\n",
        "class Item {}\n",
        "class Holder {\n",
        "    /** @var Item[] */\n",
        "    public array $items = [];\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let docblock_start = php.find("/** @var").unwrap();
    let item_in_doc = php[docblock_start..].find("Item").unwrap() + docblock_start;
    let hit = map.lookup(item_in_doc as u32);
    assert!(hit.is_some(), "Should find Item in @var Item[]");
    let span = hit.unwrap();
    let span_text = &php[span.start as usize..span.end as usize];
    assert_eq!(
        span_text, "Item",
        "Span should cover 'Item' only, not 'Item[]'"
    );
}

// ── Conditional return type tests ───────────────────────────────────

#[test]
fn conditional_return_type_all_parts_get_spans() {
    // PHPStan conditional return type:
    //   ($abstract is class-string<TClass> ? TClass : Container)
    // All three type positions (class-string<TClass>, TClass, Container)
    // should produce ClassReference spans.
    let php = concat!(
        "<?php\n",
        "class Container {\n",
        "    /**\n",
        "     * @template TClass\n",
        "     * @param string|null $abstract\n",
        "     * @return ($abstract is class-string<TClass> ? TClass : Container)\n",
        "     */\n",
        "    public function make($abstract) {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // Find offsets of each TClass and Container in the @return line.
    let return_line_start = php.find("@return").unwrap();

    // First TClass — inside class-string<TClass>
    let first_tclass_offset = php[return_line_start..].find("TClass").unwrap() + return_line_start;
    let hit1 = map.lookup(first_tclass_offset as u32);
    assert!(
        hit1.is_some(),
        "Should find first TClass (inside class-string<TClass>)"
    );
    let span1 = hit1.unwrap();
    assert_eq!(&php[span1.start as usize..span1.end as usize], "TClass");

    // Second TClass — the true branch of the conditional
    let after_first = first_tclass_offset + "TClass".len();
    let second_tclass_offset = php[after_first..].find("TClass").unwrap() + after_first;
    let hit2 = map.lookup(second_tclass_offset as u32);
    assert!(
        hit2.is_some(),
        "Should find second TClass (true branch of conditional)"
    );
    let span2 = hit2.unwrap();
    assert_eq!(&php[span2.start as usize..span2.end as usize], "TClass");
    assert_ne!(
        span1.start, span2.start,
        "The two TClass spans should be at different offsets"
    );

    // Container — the false branch of the conditional
    let container_in_return =
        php[return_line_start..].find("Container").unwrap() + return_line_start;
    let hit3 = map.lookup(container_in_return as u32);
    assert!(
        hit3.is_some(),
        "Should find Container (false branch of conditional)"
    );
    let span3 = hit3.unwrap();
    assert_eq!(&php[span3.start as usize..span3.end as usize], "Container");
}

#[test]
fn conditional_return_type_with_not_keyword() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @return ($x is not null ? Foo : Bar)\n",
        "     */\n",
        "    public function test($x) {}\n",
        "}\n",
        "class Bar {}\n",
    );
    let map = parse_and_extract(php);

    let return_start = php.find("@return").unwrap();
    let foo_in_return = php[return_start..].find("Foo").unwrap() + return_start;
    let bar_in_return = php[return_start..].find("Bar").unwrap() + return_start;

    let hit_foo = map.lookup(foo_in_return as u32);
    assert!(
        hit_foo.is_some(),
        "Should find Foo in true branch of conditional with 'is not'"
    );

    let hit_bar = map.lookup(bar_in_return as u32);
    assert!(
        hit_bar.is_some(),
        "Should find Bar in false branch of conditional with 'is not'"
    );
}

// ── var_def_kind_at tests ───────────────────────────────────────────

#[test]
fn var_def_kind_at_returns_parameter() {
    let php = concat!(
        "<?php\n",
        "class Ctrl {\n",
        "    public function handle(Request $req) {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let dollar_offset = php.find("$req").unwrap() as u32;
    let kind = map.var_def_kind_at("req", dollar_offset);
    assert_eq!(
        kind,
        Some(&VarDefKind::Parameter),
        "Should detect $req as Parameter"
    );

    let kind2 = map.var_def_kind_at("req", dollar_offset + 1);
    assert_eq!(
        kind2,
        Some(&VarDefKind::Parameter),
        "Should detect cursor on 'r' as Parameter"
    );
}

#[test]
fn var_def_kind_at_returns_catch() {
    let php = concat!(
        "<?php\n",
        "function f() {\n",
        "    try {} catch (\\Exception $e) {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let dollar_offset = php.find("$e)").unwrap() as u32;
    let kind = map.var_def_kind_at("e", dollar_offset);
    assert_eq!(kind, Some(&VarDefKind::Catch), "Should detect $e as Catch");
}

#[test]
fn var_def_kind_at_returns_foreach() {
    let php = concat!(
        "<?php\n",
        "function f() {\n",
        "    foreach ($items as $item) {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let dollar_offset = php.find("$item)").unwrap() as u32;
    let kind = map.var_def_kind_at("item", dollar_offset);
    assert_eq!(
        kind,
        Some(&VarDefKind::Foreach),
        "Should detect $item as Foreach"
    );
}

#[test]
fn var_def_kind_at_returns_none_on_usage() {
    let php = concat!(
        "<?php\n",
        "function f() {\n",
        "    $x = 1;\n",
        "    echo $x;\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let echo_x_offset = php.rfind("$x").unwrap() as u32;
    let kind = map.var_def_kind_at("x", echo_x_offset);
    assert!(kind.is_none(), "Should return None for variable usage site");
}

#[test]
fn docblock_callable_return_type_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Pencil {}\n",
        "class Factory {\n",
        "    /** @var \\Closure(): Pencil $supplier */\n",
        "    private $supplier;\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // The return type `Pencil` of `\Closure(): Pencil` should be a
    // navigable ClassReference, not swallowed into the Closure span.
    let docblock_start = php.find("/** @var").unwrap();
    let pencil_in_doc = php[docblock_start..].find("Pencil").unwrap() + docblock_start;
    let hit = map.lookup(pencil_in_doc as u32);
    assert!(hit.is_some(), "Should find Pencil in callable return type");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Pencil");
    } else {
        panic!(
            "Expected ClassReference for Pencil, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn docblock_callable_param_types_produce_class_references() {
    let php = concat!(
        "<?php\n",
        "class Request {}\n",
        "class Response {}\n",
        "class Handler {\n",
        "    /** @var callable(Request): Response $handler */\n",
        "    private $handler;\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let docblock_start = php.find("/** @var").unwrap();

    // Parameter type `Request` should be navigable.
    let request_in_doc = php[docblock_start..].find("Request").unwrap() + docblock_start;
    let hit = map.lookup(request_in_doc as u32);
    assert!(hit.is_some(), "Should find Request in callable param type");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Request");
    } else {
        panic!(
            "Expected ClassReference for Request, got {:?}",
            hit.unwrap().kind
        );
    }

    // Return type `Response` should be navigable.
    let response_in_doc = php[docblock_start..].find("Response").unwrap() + docblock_start;
    let hit = map.lookup(response_in_doc as u32);
    assert!(
        hit.is_some(),
        "Should find Response in callable return type"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Response");
    } else {
        panic!(
            "Expected ClassReference for Response, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn docblock_closure_fqn_callable_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Result {}\n",
        "class Worker {\n",
        "    /** @param \\Closure(int): Result $cb */\n",
        "    public function run($cb) {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let docblock_start = php.find("/** @param").unwrap();

    // `\Closure` should be a navigable ClassReference with is_fqn=true.
    let closure_in_doc = php[docblock_start..].find("\\Closure").unwrap() + docblock_start;
    let hit = map.lookup(closure_in_doc as u32);
    assert!(hit.is_some(), "Should find \\Closure as a ClassReference");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Closure");
        assert!(is_fqn, "\\Closure should be FQN");
    } else {
        panic!("Expected ClassReference for Closure");
    }

    // `Result` should also be navigable.
    let result_in_doc = php[docblock_start..].find("Result").unwrap() + docblock_start;
    let hit = map.lookup(result_in_doc as u32);
    assert!(hit.is_some(), "Should find Result in callable return type");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Result");
    } else {
        panic!("Expected ClassReference for Result");
    }
}

#[test]
fn template_param_in_use_tag_generic_arg() {
    // `TModel` inside `@use Foo<TModel>` should produce a ClassReference span,
    // and the template def scope should cover it so hover can resolve it.
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @template TModel of \\stdClass\n",
        " */\n",
        "class Builder {\n",
        "    /** @use SomeTrait<TModel> */\n",
        "    use SomeTrait;\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // TModel inside the generic arg should be in the symbol map.
    let use_line_offset = php.find("@use").unwrap();
    let tmodel_in_use = php[use_line_offset..].find("TModel").unwrap() + use_line_offset;
    let hit = map.lookup(tmodel_in_use as u32);
    assert!(
        hit.is_some(),
        "Should find TModel in @use generic arg in symbol map"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "TModel");
    } else {
        panic!(
            "Expected ClassReference for TModel, got {:?}",
            hit.unwrap().kind
        );
    }

    // The template def scope should cover the TModel usage.
    let found = map.find_template_def("TModel", tmodel_in_use as u32);
    assert!(
        found.is_some(),
        "Template def for TModel should cover the @use line"
    );
}

#[test]
fn static_keyword_in_generic_arg_produces_span() {
    // `static` inside `@return Builder<static>` should produce a
    // SelfStaticParent span so that hover works.
    let php = concat!(
        "<?php\n",
        "class Model {\n",
        "    /** @return Builder<static> */\n",
        "    public static function query() {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let return_line_offset = php.find("@return").unwrap();
    let static_in_generic = php[return_line_offset..].find("static").unwrap() + return_line_offset;
    let hit = map.lookup(static_in_generic as u32);
    assert!(
        hit.is_some(),
        "Should find `static` in generic arg in symbol map"
    );
    match &hit.unwrap().kind {
        SymbolKind::SelfStaticParent(kind) => {
            assert_eq!(*kind, SelfStaticParentKind::Static);
        }
        SymbolKind::ClassReference { name, .. } => {
            // `static` is in NON_NAVIGABLE, so it should NOT be a ClassReference.
            panic!(
                "static should be SelfStaticParent, not ClassReference({})",
                name
            );
        }
        other => {
            panic!("Expected SelfStaticParent for static, got {:?}", other);
        }
    }
}

#[test]
fn docblock_parenthesized_callable_in_union_produces_class_reference() {
    // `(\Closure(static): mixed)|string|array` — the outer parens are
    // grouping parens, not a conditional type.  `\Closure` inside should
    // still be recognised as a navigable ClassReference.
    let php = concat!(
        "<?php\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param  (\\Closure(static): mixed)|string|array  $column\n",
        "     * @return $this\n",
        "     */\n",
        "    public function where($column) {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let docblock_start = php.find("/**").unwrap();

    // `\Closure` inside the parenthesized callable should be a ClassReference.
    let closure_in_doc = php[docblock_start..].find("\\Closure").unwrap() + docblock_start;
    let hit = map.lookup(closure_in_doc as u32);
    assert!(
        hit.is_some(),
        "Should find \\Closure inside parenthesized callable as ClassReference"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Closure");
        assert!(is_fqn, "\\Closure should be FQN");
        // The span should cover exactly `\Closure`, not `(\Closure`.
        let span = hit.unwrap();
        let span_text = &php[span.start as usize..span.end as usize];
        assert_eq!(span_text, "\\Closure", "Span should cover only \\Closure");
    } else {
        panic!(
            "Expected ClassReference for \\Closure, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn class_const_class_in_property_default_produces_class_reference() {
    // `Foo::class` inside a property default value should produce a
    // ClassReference span for `Foo`.
    let php = concat!(
        "<?php\n",
        "class Foo {}\n",
        "class Bar {\n",
        "    protected $casts = [\n",
        "        'icing' => Foo::class,\n",
        "    ];\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let foo_in_casts = php.find("Foo::class").unwrap();
    let hit = map.lookup(foo_in_casts as u32);
    assert!(
        hit.is_some(),
        "Should find Foo in Foo::class as a ClassReference"
    );
    if let SymbolKind::ClassReference { name, .. } = &hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!(
            "Expected ClassReference for Foo, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn multiline_docblock_generic_arg_produces_class_reference() {
    // A class name inside a multiline `@return` generic type should
    // produce a ClassReference span.  Template parameters used inside
    // the multiline type should also be navigable.
    let php = concat!(
        "<?php\n",
        "class SomeCollection {}\n",
        "/**\n",
        " * @template TValue\n",
        " */\n",
        "class Demo {\n",
        "    /**\n",
        "     * @return array<\n",
        "     *   string,\n",
        "     *   SomeCollection<int, TValue>\n",
        "     * >\n",
        "     */\n",
        "    public function grouped() {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let doc_start = php.find("     * @return").unwrap();
    let some_coll = php[doc_start..].find("SomeCollection").unwrap() + doc_start;
    let hit = map.lookup(some_coll as u32);
    assert!(
        hit.is_some(),
        "Should find SomeCollection in multiline @return generic arg"
    );
    let span = hit.unwrap();
    if let SymbolKind::ClassReference { name, .. } = &span.kind {
        assert_eq!(name, "SomeCollection");
        // Verify the span text matches the original source exactly.
        let span_text = &php[span.start as usize..span.end as usize];
        assert_eq!(
            span_text, "SomeCollection",
            "Span text should be exactly 'SomeCollection', got {:?}",
            span_text
        );
    } else {
        panic!(
            "Expected ClassReference for SomeCollection, got {:?}",
            span.kind
        );
    }

    // TValue inside the multiline generic arg should also be navigable.
    let tvalue_pos = php[doc_start..].find("TValue").unwrap() + doc_start;
    let hit2 = map.lookup(tvalue_pos as u32);
    assert!(
        hit2.is_some(),
        "Should find TValue in multiline @return generic arg"
    );
    let span2 = hit2.unwrap();
    if let SymbolKind::ClassReference { name, .. } = &span2.kind {
        assert_eq!(name, "TValue");
        let span_text = &php[span2.start as usize..span2.end as usize];
        assert_eq!(
            span_text, "TValue",
            "Span text should be exactly 'TValue', got {:?}",
            span_text
        );
    } else {
        panic!("Expected ClassReference for TValue, got {:?}", span2.kind);
    }

    // The template def scope should cover the TValue usage.
    let found = map.find_template_def("TValue", tvalue_pos as u32);
    assert!(
        found.is_some(),
        "Template def for TValue should cover the multiline @return usage"
    );
}

#[test]
fn phpstan_assert_tag_produces_class_reference() {
    // `@phpstan-assert-if-false Rock $value` should produce a
    // ClassReference span for `Rock`.
    let php = concat!(
        "<?php\n",
        "class Rock {}\n",
        "class Checker {\n",
        "    /** @phpstan-assert-if-false Rock $value */\n",
        "    public function isValid($value): bool { return true; }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let doc_start = php.find("@phpstan-assert-if-false").unwrap();
    let rock_pos = php[doc_start..].find("Rock").unwrap() + doc_start;
    let hit = map.lookup(rock_pos as u32);
    assert!(
        hit.is_some(),
        "Should find Rock in @phpstan-assert-if-false as a ClassReference"
    );
    let span = hit.unwrap();
    if let SymbolKind::ClassReference { name, .. } = &span.kind {
        assert_eq!(name, "Rock");
        let span_text = &php[span.start as usize..span.end as usize];
        assert_eq!(span_text, "Rock");
    } else {
        panic!("Expected ClassReference for Rock, got {:?}", span.kind);
    }
}

// ── Top-level const statement ───────────────────────────────────────────────

#[test]
fn top_level_const_value_produces_class_reference() {
    let php = "<?php\nconst MY_CLASS = Foo::class;\n";
    let map = parse_and_extract(php);
    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some(), "Should find Foo in const value expression");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!(
            "Expected ClassReference for Foo, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn top_level_const_multiple_items() {
    let php = "<?php\nconst A = Foo::class, B = Bar::class;\n";
    let map = parse_and_extract(php);

    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit_foo = map.lookup(foo_offset);
    assert!(hit_foo.is_some(), "Should find Foo in first const item");
    if let SymbolKind::ClassReference { ref name, .. } = hit_foo.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for Foo");
    }

    let bar_offset = php.find("Bar").unwrap() as u32;
    let hit_bar = map.lookup(bar_offset);
    assert!(hit_bar.is_some(), "Should find Bar in second const item");
    if let SymbolKind::ClassReference { ref name, .. } = hit_bar.unwrap().kind {
        assert_eq!(name, "Bar");
    } else {
        panic!("Expected ClassReference for Bar");
    }
}

// ── Anonymous class ─────────────────────────────────────────────────────────

#[test]
fn anonymous_class_extends_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "function test() {\n",
        "    return new class extends Foo implements Bar {};\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some(), "Should find Foo in anonymous class extends");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for Foo");
    }

    let bar_offset = php.find("Bar").unwrap() as u32;
    let hit2 = map.lookup(bar_offset);
    assert!(
        hit2.is_some(),
        "Should find Bar in anonymous class implements"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit2.unwrap().kind {
        assert_eq!(name, "Bar");
    } else {
        panic!("Expected ClassReference for Bar");
    }
}

#[test]
fn anonymous_class_members_are_extracted() {
    let php = concat!(
        "<?php\n",
        "function test() {\n",
        "    return new class {\n",
        "        public function run(Baz $b) {}\n",
        "    };\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let baz_offset = php.find("Baz").unwrap() as u32;
    let hit = map.lookup(baz_offset);
    assert!(
        hit.is_some(),
        "Should find Baz in anonymous class method param type"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Baz");
    } else {
        panic!("Expected ClassReference for Baz");
    }
}

#[test]
fn anonymous_class_constructor_args_are_extracted() {
    let php = concat!(
        "<?php\n",
        "function test() {\n",
        "    return new class(new Qux()) {};\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let qux_offset = php.find("Qux").unwrap() as u32;
    let hit = map.lookup(qux_offset);
    assert!(
        hit.is_some(),
        "Should find Qux in anonymous class constructor arg"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Qux");
    } else {
        panic!("Expected ClassReference for Qux");
    }
}

// ── Language constructs ─────────────────────────────────────────────────────

#[test]
fn construct_isset_extracts_inner_expressions() {
    let php = "<?php\nfunction t() { isset($foo->bar); }\n";
    let map = parse_and_extract(php);

    let bar_offset = php.find("bar").unwrap() as u32;
    let hit = map.lookup(bar_offset);
    assert!(hit.is_some(), "Should find bar in isset()");
    if let SymbolKind::MemberAccess {
        ref member_name, ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "bar");
    } else {
        panic!("Expected MemberAccess for bar, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn construct_empty_extracts_inner_expression() {
    let php = "<?php\nfunction t() { empty($foo->name); }\n";
    let map = parse_and_extract(php);

    let name_offset = php.find("name").unwrap() as u32;
    let hit = map.lookup(name_offset);
    assert!(hit.is_some(), "Should find name in empty()");
    if let SymbolKind::MemberAccess {
        ref member_name, ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "name");
    } else {
        panic!(
            "Expected MemberAccess for name, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn construct_print_extracts_inner_expression() {
    let php = "<?php\nfunction t(Foo $x) { print $x->label; }\n";
    let map = parse_and_extract(php);

    let label_offset = php.find("label").unwrap() as u32;
    let hit = map.lookup(label_offset);
    assert!(hit.is_some(), "Should find label in print expression");
    if let SymbolKind::MemberAccess {
        ref member_name, ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "label");
    } else {
        panic!("Expected MemberAccess for label");
    }
}

// ── Composite strings (interpolation) ───────────────────────────────────────

#[test]
fn composite_string_extracts_variable() {
    let php = "<?php\nfunction t() { $x = 1; echo \"val={$x}\"; }\n";
    let map = parse_and_extract(php);

    // The $x inside the interpolated string should have a Variable span.
    // Find the second occurrence of $x (the one inside the string).
    let first_x = php.find("$x").unwrap();
    let second_x = php[first_x + 2..].find("$x").unwrap() + first_x + 2;
    let hit = map.lookup(second_x as u32);
    assert!(hit.is_some(), "Should find $x inside interpolated string");
    if let SymbolKind::Variable { ref name } = hit.unwrap().kind {
        assert_eq!(name, "x");
    } else {
        panic!("Expected Variable for $x, got {:?}", hit.unwrap().kind);
    }
}

#[test]
fn composite_string_braced_expression_extracts_member_access() {
    let php = "<?php\nfunction t() { echo \"name={$obj->name}\"; }\n";
    let map = parse_and_extract(php);

    let name_offset = php.find("name}").unwrap() as u32;
    let hit = map.lookup(name_offset);
    assert!(
        hit.is_some(),
        "Should find member access inside braced string expression"
    );
    if let SymbolKind::MemberAccess {
        ref member_name, ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "name");
    } else {
        panic!(
            "Expected MemberAccess for name, got {:?}",
            hit.unwrap().kind
        );
    }
}

// ── Array append ────────────────────────────────────────────────────────────

#[test]
fn array_append_extracts_array_variable() {
    let php = "<?php\nfunction t() { $arr[] = 1; }\n";
    let map = parse_and_extract(php);

    let arr_offset = php.find("$arr").unwrap() as u32;
    let hit = map.lookup(arr_offset);
    assert!(hit.is_some(), "Should find $arr in array append LHS");
    if let SymbolKind::Variable { ref name } = hit.unwrap().kind {
        assert_eq!(name, "arr");
    } else {
        panic!("Expected Variable for $arr, got {:?}", hit.unwrap().kind);
    }
}

// ── Standalone constant access ──────────────────────────────────────────────

#[test]
fn constant_access_produces_constant_reference() {
    let php = "<?php\nfunction t() { echo PHP_EOL; }\n";
    let map = parse_and_extract(php);

    let eol_offset = php.find("PHP_EOL").unwrap() as u32;
    let hit = map.lookup(eol_offset);
    assert!(hit.is_some(), "Should find PHP_EOL as ConstantReference");
    if let SymbolKind::ConstantReference { ref name } = hit.unwrap().kind {
        assert_eq!(name, "PHP_EOL");
    } else {
        panic!(
            "Expected ConstantReference for PHP_EOL, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn static_class_constant_access_produces_class_reference() {
    // `\App\MyClass::FOO` — the class part should be a ClassReference.
    let php = "<?php\nfunction t() { echo \\App\\MyClass::FOO; }\n";
    let map = parse_and_extract(php);

    let class_offset = php.find("\\App\\MyClass").unwrap() as u32;
    let hit = map.lookup(class_offset);
    assert!(hit.is_some(), "Should find \\App\\MyClass");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "App\\MyClass");
        assert!(is_fqn);
    } else {
        panic!(
            "Expected ClassReference for App\\MyClass, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn namespaced_standalone_constant_produces_constant_reference() {
    // A standalone namespaced constant like `PHPStan\PHP_VERSION_ID` is a
    // ConstantAccess in the parser — it must NOT be emitted as a
    // ClassReference, which would cause false "unknown class" diagnostics.
    let php = "<?php\nfunction t() { echo \\PHPStan\\PHP_VERSION_ID; }\n";
    let map = parse_and_extract(php);

    let const_offset = php.find("\\PHPStan\\PHP_VERSION_ID").unwrap() as u32;
    let hit = map.lookup(const_offset);
    assert!(
        hit.is_some(),
        "Should find \\PHPStan\\PHP_VERSION_ID as ConstantReference"
    );
    if let SymbolKind::ConstantReference { ref name } = hit.unwrap().kind {
        assert_eq!(name, "PHPStan\\PHP_VERSION_ID");
    } else {
        panic!(
            "Expected ConstantReference for PHPStan\\PHP_VERSION_ID, got {:?}",
            hit.unwrap().kind
        );
    }
}

// ── Pipe operator ───────────────────────────────────────────────────────────

// Note: PHP 8.5 pipe operator support. The parser may or may not handle
// this syntax depending on the mago_syntax version. The extraction code
// is in place for when it does.

// ── First-class callable / partial application ──────────────────────────────

#[test]
fn first_class_callable_function_produces_function_call() {
    let php = "<?php\nfunction t() { $fn = strlen(...); }\n";
    let map = parse_and_extract(php);

    let strlen_offset = php.find("strlen").unwrap() as u32;
    let hit = map.lookup(strlen_offset);
    assert!(hit.is_some(), "Should find strlen in first-class callable");
    if let SymbolKind::FunctionCall { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "strlen");
    } else {
        panic!(
            "Expected FunctionCall for strlen, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn first_class_callable_static_method_produces_member_access() {
    let php = "<?php\nfunction t() { $fn = Foo::bar(...); }\n";
    let map = parse_and_extract(php);

    // `Foo` should be a ClassReference.
    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit_foo = map.lookup(foo_offset);
    assert!(hit_foo.is_some(), "Should find Foo class reference");
    if let SymbolKind::ClassReference { ref name, .. } = hit_foo.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for Foo");
    }

    // `bar` should be a MemberAccess.
    let bar_offset = php.find("bar").unwrap() as u32;
    let hit_bar = map.lookup(bar_offset);
    assert!(
        hit_bar.is_some(),
        "Should find bar in static first-class callable"
    );
    if let SymbolKind::MemberAccess {
        ref member_name,
        is_static,
        is_method_call,
        ..
    } = hit_bar.unwrap().kind
    {
        assert_eq!(member_name, "bar");
        assert!(is_static);
        assert!(is_method_call);
    } else {
        panic!(
            "Expected MemberAccess for bar, got {:?}",
            hit_bar.unwrap().kind
        );
    }
}

#[test]
fn first_class_callable_instance_method_produces_member_access() {
    let php = "<?php\nfunction t($obj) { $fn = $obj->baz(...); }\n";
    let map = parse_and_extract(php);

    let baz_offset = php.find("baz").unwrap() as u32;
    let hit = map.lookup(baz_offset);
    assert!(
        hit.is_some(),
        "Should find baz in instance first-class callable"
    );
    if let SymbolKind::MemberAccess {
        ref member_name,
        is_static,
        is_method_call,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "baz");
        assert!(!is_static);
        assert!(is_method_call);
    } else {
        panic!("Expected MemberAccess for baz, got {:?}", hit.unwrap().kind);
    }
}

// ── Echo tag (short open tag) ───────────────────────────────────────────────

// Note: `<?= $expr ?>` parsing depends on the parser treating it as an
// EchoTag statement. The extraction code handles it; a full integration
// test would require a PHP file with short echo tags that the parser
// recognises.

// ── Declare statement ───────────────────────────────────────────────────────

#[test]
fn declare_statement_body_extracts_symbols() {
    let php = concat!(
        "<?php\n",
        "declare(strict_types=1);\n",
        "function test(Foo $x) {}\n",
    );
    let map = parse_and_extract(php);

    let foo_offset = php.find("Foo").unwrap() as u32;
    let hit = map.lookup(foo_offset);
    assert!(hit.is_some(), "Should find Foo after declare statement");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Foo");
    } else {
        panic!("Expected ClassReference for Foo");
    }
}

#[test]
fn docblock_array_shape_value_type_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Pen {}\n",
        "/**\n",
        " * @return array{logger: Pen, debug: bool}\n",
        " */\n",
        "function getConfig(): array { return []; }\n",
    );
    let map = parse_and_extract(php);

    let pen_offset = php.find("Pen, debug").unwrap() as u32;
    let hit = map.lookup(pen_offset);
    assert!(
        hit.is_some(),
        "Should find Pen inside array shape value type"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Pen");
    } else {
        panic!(
            "Expected ClassReference for Pen, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn docblock_array_shape_multiple_class_values() {
    let php = concat!(
        "<?php\n",
        "class Logger {}\n",
        "class Mailer {}\n",
        "/**\n",
        " * @return array{log: Logger, mail: Mailer}\n",
        " */\n",
        "function services(): array { return []; }\n",
    );
    let map = parse_and_extract(php);

    // Check Logger
    let logger_offset = php.find("Logger, mail").unwrap() as u32;
    let hit = map.lookup(logger_offset);
    assert!(hit.is_some(), "Should find Logger in array shape");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Logger");
    } else {
        panic!("Expected ClassReference for Logger");
    }

    // Check Mailer
    let mailer_offset = php.find("Mailer}").unwrap() as u32;
    let hit = map.lookup(mailer_offset);
    assert!(hit.is_some(), "Should find Mailer in array shape");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Mailer");
    } else {
        panic!("Expected ClassReference for Mailer");
    }
}

#[test]
fn docblock_object_shape_value_type_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "class User {}\n",
        "/**\n",
        " * @return object{owner: User, active: bool}\n",
        " */\n",
        "function getProfile(): object { return (object)[]; }\n",
    );
    let map = parse_and_extract(php);

    let user_offset = php.find("User, active").unwrap() as u32;
    let hit = map.lookup(user_offset);
    assert!(
        hit.is_some(),
        "Should find User inside object shape value type"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "User");
    } else {
        panic!("Expected ClassReference for User");
    }
}

#[test]
fn docblock_array_shape_scalar_value_not_navigable() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @return array{name: string, count: int}\n",
        " */\n",
        "function getData(): array { return []; }\n",
    );
    let map = parse_and_extract(php);

    let string_offset = php.find("string, count").unwrap() as u32;
    let hit = map.lookup(string_offset);
    assert!(
        hit.is_none(),
        "Scalar 'string' inside array shape should not be navigable"
    );
}

#[test]
fn docblock_array_shape_optional_key_value_produces_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Widget {}\n",
        "/**\n",
        " * @return array{item?: Widget}\n",
        " */\n",
        "function maybeWidget(): array { return []; }\n",
    );
    let map = parse_and_extract(php);

    let widget_offset = php.find("Widget}").unwrap() as u32;
    let hit = map.lookup(widget_offset);
    assert!(
        hit.is_some(),
        "Should find Widget in optional array shape entry"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Widget");
    } else {
        panic!("Expected ClassReference for Widget");
    }
}

#[test]
fn docblock_array_shape_nested_generic_value() {
    let php = concat!(
        "<?php\n",
        "class Item {}\n",
        "/**\n",
        " * @return array{items: list<Item>, total: int}\n",
        " */\n",
        "function paginated(): array { return []; }\n",
    );
    let map = parse_and_extract(php);

    let item_offset = php.find("Item>, total").unwrap() as u32;
    let hit = map.lookup(item_offset);
    assert!(
        hit.is_some(),
        "Should find Item inside generic within array shape"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Item");
    } else {
        panic!("Expected ClassReference for Item");
    }
}

#[test]
fn arrow_fn_body_scope_suppresses_outer_call_site() {
    let src = concat!(
        "<?php\n",
        "class Collection {\n",
        "    /** @param callable(mixed): mixed $callback */\n",
        "    public function each(callable $callback): static { return $this; }\n",
        "}\n",
        "class ReviewModel {\n",
        "    public float $percentage = 0.0;\n",
        "    public int $count = 0;\n",
        "}\n",
        "$ratings = new Collection();\n",
        "$total = 10;\n",
        "$ratings->each(fn(ReviewModel $model): float => $model->percentage = $total > 0 ? ($model->count / $total) * 100 : 0.0);\n",
    );
    let sm = parse_and_extract(src);

    // Find the `each(` call site.
    let each_cs = sm
        .call_sites
        .iter()
        .find(|cs| cs.call_expression.contains("each"))
        .expect("should have a call site for each()");

    // The cursor at col 60 on line 11 is inside the arrow fn body.
    // Compute a byte offset in the arrow fn body region (past `=>`).
    let arrow_body_marker = src.find("$model->percentage").unwrap() as u32;

    // There should be a body scope (arrow fn `=>`) starting inside each()'s args.
    let has_nested = sm.is_inside_nested_scope_of_call(arrow_body_marker, each_cs);
    assert!(
        has_nested,
        "Cursor at offset {} should be inside a nested body scope of each() call (args {}..{}), body_scopes: {:?}",
        arrow_body_marker, each_cs.args_start, each_cs.args_end, sm.body_scopes,
    );
}

// ── @see tag symbol extraction tests ────────────────────────────────────────

#[test]
fn see_tag_class_reference() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see UserService\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("UserService").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some(), "Should find UserService from @see tag");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "UserService");
    } else {
        panic!("Expected ClassReference for @see UserService");
    }
}

#[test]
fn see_tag_fqn_class_reference() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see \\App\\Models\\User\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("\\App\\Models\\User").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some(), "Should find FQN class from @see tag");
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "App\\Models\\User");
        assert!(is_fqn, "Leading backslash should set is_fqn");
    } else {
        panic!("Expected ClassReference for @see \\App\\Models\\User");
    }
}

#[test]
fn see_tag_member_method() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see Order::getTotal()\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    // The class part should produce a ClassReference.
    let class_offset = php.find("Order").unwrap() as u32;
    let hit = map.lookup(class_offset);
    assert!(
        hit.is_some(),
        "Should find Order from @see Order::getTotal()"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Order");
    } else {
        panic!("Expected ClassReference for Order");
    }

    // The member part should produce a MemberAccess.
    let member_offset = php.find("getTotal").unwrap() as u32;
    let hit = map.lookup(member_offset);
    assert!(
        hit.is_some(),
        "Should find getTotal from @see Order::getTotal()"
    );
    if let SymbolKind::MemberAccess {
        ref subject_text,
        ref member_name,
        is_static,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(subject_text, "Order");
        assert_eq!(member_name, "getTotal");
        assert!(is_static, "@see members are treated as static access");
    } else {
        panic!("Expected MemberAccess for getTotal");
    }
}

#[test]
fn see_tag_member_property() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see Order::$channel_type\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    // The class part should produce a ClassReference.
    let class_offset = php.find("Order").unwrap() as u32;
    let hit = map.lookup(class_offset);
    assert!(hit.is_some(), "Should find Order from @see tag");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Order");
    } else {
        panic!("Expected ClassReference for Order");
    }

    // The $channel_type part should produce a MemberAccess.
    let prop_offset = php.find("$channel_type").unwrap() as u32;
    let hit = map.lookup(prop_offset);
    assert!(hit.is_some(), "Should find $channel_type from @see tag");
    if let SymbolKind::MemberAccess {
        ref subject_text,
        ref member_name,
        is_static,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(subject_text, "Order");
        assert_eq!(member_name, "channel_type");
        assert!(is_static);
    } else {
        panic!("Expected MemberAccess for $channel_type");
    }
}

#[test]
fn see_tag_member_constant() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see Order::STATUS_PENDING\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let const_offset = php.find("STATUS_PENDING").unwrap() as u32;
    let hit = map.lookup(const_offset);
    assert!(
        hit.is_some(),
        "Should find STATUS_PENDING from @see Order::STATUS_PENDING"
    );
    if let SymbolKind::MemberAccess {
        ref subject_text,
        ref member_name,
        is_static,
        ..
    } = hit.unwrap().kind
    {
        assert_eq!(subject_text, "Order");
        assert_eq!(member_name, "STATUS_PENDING");
        assert!(is_static);
    } else {
        panic!("Expected MemberAccess for STATUS_PENDING");
    }
}

#[test]
fn see_tag_url_skipped() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see https://example.com/docs\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("https://example.com").unwrap() as u32;
    let hit = map.lookup(offset);
    // URLs should NOT produce a symbol span.
    assert!(
        hit.is_none()
            || !matches!(
                hit.unwrap().kind,
                SymbolKind::ClassReference { .. } | SymbolKind::FunctionCall { .. }
            ),
        "URL in @see should not produce a navigable symbol"
    );
}

#[test]
fn see_tag_function_reference() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see fixture()\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("fixture").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some(), "Should find fixture from @see fixture()");
    if let SymbolKind::FunctionCall { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "fixture");
    } else {
        panic!(
            "Expected FunctionCall for fixture, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn see_tag_inline_form() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * Wraps {@see UserService} with caching.\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("UserService").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some(), "Should find UserService from inline @see");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "UserService");
    } else {
        panic!("Expected ClassReference for inline @see UserService");
    }
}

#[test]
fn see_tag_inline_function_reference() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * Wraps {@see fixture()} with extra logic.\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("fixture").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(
        hit.is_some(),
        "Should find fixture from inline {{@see fixture()}}"
    );
    if let SymbolKind::FunctionCall { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "fixture");
    } else {
        panic!(
            "Expected FunctionCall for inline @see fixture(), got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn see_tag_inline_member_reference() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * Uses {@see Order::getTotal()} internally.\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let class_offset = php.find("Order").unwrap() as u32;
    let hit = map.lookup(class_offset);
    assert!(hit.is_some(), "Should find Order from inline @see");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Order");
    } else {
        panic!("Expected ClassReference for Order in inline @see");
    }

    let member_offset = php.find("getTotal").unwrap() as u32;
    let hit = map.lookup(member_offset);
    assert!(hit.is_some(), "Should find getTotal from inline @see");
    if let SymbolKind::MemberAccess {
        ref member_name, ..
    } = hit.unwrap().kind
    {
        assert_eq!(member_name, "getTotal");
    } else {
        panic!("Expected MemberAccess for getTotal in inline @see");
    }
}

#[test]
fn see_tag_scalar_type_not_navigable() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see string\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    // Find the "string" that is part of the @see tag, not the one
    // from the class keyword.
    let see_line = php.find("@see string").unwrap();
    let string_offset = (see_line + "@see ".len()) as u32;
    let hit = map.lookup(string_offset);
    assert!(
        hit.is_none()
            || !matches!(
                hit.unwrap().kind,
                SymbolKind::ClassReference { .. } | SymbolKind::FunctionCall { .. }
            ),
        "Scalar type 'string' in @see should not produce a navigable symbol"
    );
}

#[test]
fn see_tag_multiple_on_same_docblock() {
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see UserService\n",
        " * @see OrderService\n",
        " */\n",
        "class Foo {}\n",
    );
    let map = parse_and_extract(php);

    let user_offset = php.find("UserService").unwrap() as u32;
    let hit = map.lookup(user_offset);
    assert!(hit.is_some(), "Should find UserService");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "UserService");
    } else {
        panic!("Expected ClassReference for UserService");
    }

    let order_offset = php.find("OrderService").unwrap() as u32;
    let hit = map.lookup(order_offset);
    assert!(hit.is_some(), "Should find OrderService");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "OrderService");
    } else {
        panic!("Expected ClassReference for OrderService");
    }
}

#[test]
fn see_tag_qualified_member_spans_aligned() {
    // A qualified-but-not-FQN reference (`App\Foo::bar()`) gets a synthetic
    // leading `\` for FQN handling. The emitted spans must still align with
    // the original source bytes, not be shifted by the synthetic prefix.
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see App\\Foo::bar()\n",
        " */\n",
        "class Baz {}\n",
    );
    let map = parse_and_extract(php);

    // Class portion: span must start at `App` and end right after `App\Foo`.
    let class_offset = php.find("App\\Foo").unwrap() as u32;
    let hit = map
        .lookup(class_offset)
        .expect("Should find App\\Foo class");
    assert_eq!(hit.start, class_offset, "class span start aligned");
    assert_eq!(
        hit.end,
        class_offset + "App\\Foo".len() as u32,
        "class span end must not overshoot by the synthetic prefix"
    );

    // Member portion: lookup at the exact `bar` offset must succeed (a
    // one-byte shift would make this miss).
    let member_offset = php.find("bar").unwrap() as u32;
    let hit = map.lookup(member_offset).expect("Should find bar member");
    assert_eq!(hit.start, member_offset, "member span start aligned");
    assert_eq!(
        hit.end,
        member_offset + "bar".len() as u32,
        "member span end aligned"
    );
    if let SymbolKind::MemberAccess {
        ref member_name, ..
    } = hit.kind
    {
        assert_eq!(member_name, "bar");
    } else {
        panic!("Expected MemberAccess for bar");
    }
}

#[test]
fn see_tag_qualified_class_span_aligned() {
    // A bare qualified class reference (no `::`) also gets the synthetic
    // prefix; its span must not overshoot.
    let php = concat!(
        "<?php\n",
        "/**\n",
        " * @see App\\Models\\User\n",
        " */\n",
        "class Baz {}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("App\\Models\\User").unwrap() as u32;
    let hit = map.lookup(offset).expect("Should find App\\Models\\User");
    assert_eq!(hit.start, offset, "class span start aligned");
    assert_eq!(
        hit.end,
        offset + "App\\Models\\User".len() as u32,
        "class span end must not overshoot by the synthetic prefix"
    );
}

#[test]
fn see_tag_on_method_docblock() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @see BarService\n",
        "     */\n",
        "    public function test() {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("BarService").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(hit.is_some(), "Should find BarService on method docblock");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "BarService");
    } else {
        panic!("Expected ClassReference for BarService");
    }
}

#[test]
fn see_tag_on_property_docblock() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @see CacheDriver\n",
        "     */\n",
        "    private $cache;\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let offset = php.find("CacheDriver").unwrap() as u32;
    let hit = map.lookup(offset);
    assert!(
        hit.is_some(),
        "Should find CacheDriver on property docblock"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "CacheDriver");
    } else {
        panic!("Expected ClassReference for CacheDriver");
    }
}

#[test]
fn property_attribute_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    #[\\Assert\\NotBlank]\n",
        "    public string $name;\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let attr_offset = php.find("\\Assert\\NotBlank").unwrap() as u32;
    let hit = map.lookup(attr_offset);
    assert!(
        hit.is_some(),
        "Should find attribute class reference on property"
    );
    if let SymbolKind::ClassReference {
        ref name, is_fqn, ..
    } = hit.unwrap().kind
    {
        assert_eq!(name, "Assert\\NotBlank");
        assert!(is_fqn);
    } else {
        panic!(
            "Expected ClassReference for property attribute, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn hooked_property_attribute_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    #[\\Assert\\Email]\n",
        "    public string $email {\n",
        "        set { $this->email = strtolower($value); }\n",
        "    }\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let attr_offset = php.find("\\Assert\\Email").unwrap() as u32;
    let hit = map.lookup(attr_offset);
    assert!(
        hit.is_some(),
        "Should find attribute class reference on hooked property"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Assert\\Email");
    } else {
        panic!(
            "Expected ClassReference for hooked property attribute, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn class_constant_attribute_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    #[\\Deprecated]\n",
        "    const OLD_NAME = 'old';\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let attr_offset = php.find("\\Deprecated").unwrap() as u32;
    let hit = map.lookup(attr_offset);
    assert!(
        hit.is_some(),
        "Should find attribute class reference on class constant"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Deprecated");
    } else {
        panic!(
            "Expected ClassReference for constant attribute, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn parameter_attribute_class_reference() {
    let php = concat!(
        "<?php\n",
        "function handle(#[\\SensitiveParameter] string $secret): void {}\n",
    );
    let map = parse_and_extract(php);

    let attr_offset = php.find("\\SensitiveParameter").unwrap() as u32;
    let hit = map.lookup(attr_offset);
    assert!(
        hit.is_some(),
        "Should find attribute class reference on parameter"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "SensitiveParameter");
    } else {
        panic!(
            "Expected ClassReference for parameter attribute, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn method_parameter_attribute_class_reference() {
    let php = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public function run(#[\\Autowire] Logger $log): void {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let attr_offset = php.find("\\Autowire").unwrap() as u32;
    let hit = map.lookup(attr_offset);
    assert!(
        hit.is_some(),
        "Should find attribute class reference on method parameter"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Autowire");
    } else {
        panic!(
            "Expected ClassReference for method parameter attribute, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn enum_case_attribute_class_reference() {
    let php = concat!(
        "<?php\n",
        "enum Status: string {\n",
        "    #[\\Deprecated]\n",
        "    case Legacy = 'legacy';\n",
        "    case Active = 'active';\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let attr_offset = php.find("\\Deprecated").unwrap() as u32;
    let hit = map.lookup(attr_offset);
    assert!(
        hit.is_some(),
        "Should find attribute class reference on enum case"
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Deprecated");
    } else {
        panic!(
            "Expected ClassReference for enum case attribute, got {:?}",
            hit.unwrap().kind
        );
    }
}

#[test]
fn multiple_property_attributes() {
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    #[\\Assert\\NotBlank, \\Assert\\Length(max: 255)]\n",
        "    public string $title;\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    let nb_offset = php.find("\\Assert\\NotBlank").unwrap() as u32;
    let hit = map.lookup(nb_offset);
    assert!(hit.is_some(), "Should find first property attribute");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Assert\\NotBlank");
    } else {
        panic!("Expected ClassReference for first attribute");
    }

    let len_offset = php.find("\\Assert\\Length").unwrap() as u32;
    let hit = map.lookup(len_offset);
    assert!(hit.is_some(), "Should find second property attribute");
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Assert\\Length");
    } else {
        panic!("Expected ClassReference for second attribute");
    }
}

#[test]
fn docblock_star_wildcard_in_generic_produces_correct_spans() {
    // PHPStan `*` wildcards in generic positions must not cause the
    // entire type string to become a single broken ClassReference.
    // Instead, the base class and other named generic args should
    // produce individual navigable spans.
    let php = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @param Relation<Model, *, *>|string $rel\n",
        "     */\n",
        "    public function bar($rel): void {}\n",
        "}\n",
    );
    let map = parse_and_extract(php);

    // Find the `Relation` class reference span.
    let relation_offset = php.find("Relation<").unwrap() as u32;
    let hit = map.lookup(relation_offset);
    assert!(
        hit.is_some(),
        "Should find ClassReference for Relation at offset {}",
        relation_offset
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Relation", "Base class name should be 'Relation'");
    } else {
        panic!(
            "Expected ClassReference for Relation, got {:?}",
            hit.unwrap().kind
        );
    }

    // Find the `Model` class reference inside the generic args.
    let model_offset = php.find("Model, *").unwrap() as u32;
    let hit = map.lookup(model_offset);
    assert!(
        hit.is_some(),
        "Should find ClassReference for Model at offset {}",
        model_offset
    );
    if let SymbolKind::ClassReference { ref name, .. } = hit.unwrap().kind {
        assert_eq!(name, "Model", "Generic arg should be 'Model'");
    } else {
        panic!(
            "Expected ClassReference for Model, got {:?}",
            hit.unwrap().kind
        );
    }

    // Verify that no span has the full unparsed type string as its name
    // (this was the bug: the entire "Relation<Model, *, *>|string" was
    // emitted as one ClassReference).
    for span in &map.spans {
        if let SymbolKind::ClassReference { ref name, .. } = span.kind {
            assert!(
                !name.contains('<') && !name.contains('*'),
                "ClassReference name must not contain raw generic syntax or wildcards, got: {}",
                name
            );
        }
    }
}

// ── array-callable spans ────────────────────────────────────────────

#[test]
fn array_callable_class_const_emits_member_access() {
    // `[Controller::class, 'method']` — the Laravel route/controller-action
    // shape. Cursor on the method string should resolve as a member access.
    let php = "<?php\nRoute::get('/', [IndexPageController::class, 'indexPage']);\n";
    let map = parse_and_extract(php);
    let offset = php.find("indexPage']").unwrap() as u32;
    let hit = map.lookup(offset).expect("span at method string");
    match &hit.kind {
        SymbolKind::MemberAccess {
            subject_text,
            member_name,
            is_static,
            is_method_call,
            ..
        } => {
            assert_eq!(subject_text, "IndexPageController");
            assert_eq!(member_name, "indexPage");
            assert!(*is_static);
            assert!(*is_method_call);
        }
        other => panic!("Expected MemberAccess, got {:?}", other),
    }
}

#[test]
fn array_callable_variable_emits_instance_member_access() {
    // `[$this, 'method']` and `[$obj, 'method']` — instance-style callables.
    let php = "<?php\n$cb = [$this, 'handle'];\n";
    let map = parse_and_extract(php);
    let offset = php.find("handle']").unwrap() as u32;
    let hit = map.lookup(offset).expect("span at method string");
    match &hit.kind {
        SymbolKind::MemberAccess {
            subject_text,
            member_name,
            is_static,
            ..
        } => {
            assert_eq!(subject_text, "$this");
            assert_eq!(member_name, "handle");
            assert!(!*is_static);
        }
        other => panic!("Expected MemberAccess, got {:?}", other),
    }
}

#[test]
fn plain_two_string_array_is_not_a_callable() {
    // `['foo', 'bar']` — a plain data array, not a callable. No span should
    // be emitted over the second string.
    let php = "<?php\n$x = ['foo', 'bar'];\n";
    let map = parse_and_extract(php);
    let offset = php.find("bar']").unwrap() as u32;
    assert!(
        map.lookup(offset).is_none(),
        "plain string array must not emit a member-access span"
    );
}

#[test]
fn array_callable_ignores_non_identifier_method_string() {
    // `[$this, 'not a method']` — the second string is not an identifier.
    let php = "<?php\n$x = [$this, 'not a method'];\n";
    let map = parse_and_extract(php);
    let offset = php.find("not a method").unwrap() as u32;
    assert!(
        map.lookup(offset).is_none(),
        "non-identifier string must not emit a member-access span"
    );
}
