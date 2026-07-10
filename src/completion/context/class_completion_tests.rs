use super::class_edit_texts;
use super::*;
use crate::types::ClassLikeKind;

// ── detect_stub_class_kind ──────────────────────────────────────

#[test]
fn test_detect_class_in_single_class_file() {
    let source = "<?php\nclass DateTime {\n}\n";
    let result = detect_stub_class_kind("DateTime", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Class, false, false)),
        "should detect a plain class"
    );
}

#[test]
fn test_detect_interface_in_single_file() {
    let source = "<?php\ninterface JsonSerializable\n{\n}\n";
    let result = detect_stub_class_kind("JsonSerializable", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Interface, false, false)),
        "should detect an interface"
    );
}

#[test]
fn test_detect_abstract_class() {
    let source = "<?php\nabstract class SplHeap implements Iterator, Countable\n{\n}\n";
    let result = detect_stub_class_kind("SplHeap", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Class, true, false)),
        "should detect an abstract class"
    );
}

#[test]
fn test_detect_final_class() {
    let source = "<?php\nfinal class Closure {\n}\n";
    let result = detect_stub_class_kind("Closure", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Class, false, true)),
        "should detect a final class"
    );
}

#[test]
fn test_detect_readonly_class() {
    let source = "<?php\nreadonly class Value {\n}\n";
    let result = detect_stub_class_kind("Value", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Class, false, false)),
        "readonly class is neither abstract nor final"
    );
}

#[test]
fn test_detect_final_readonly_class() {
    let source = "<?php\nfinal readonly class Immutable {\n}\n";
    let result = detect_stub_class_kind("Immutable", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Class, false, true)),
        "should detect final through readonly"
    );
}

#[test]
fn test_detect_abstract_readonly_class() {
    let source = "<?php\nabstract readonly class Base {\n}\n";
    let result = detect_stub_class_kind("Base", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Class, true, false)),
        "should detect abstract through readonly"
    );
}

#[test]
fn test_detect_trait() {
    let source = "<?php\ntrait Stringable {\n}\n";
    let result = detect_stub_class_kind("Stringable", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Trait, false, false)),
        "should detect a trait"
    );
}

#[test]
fn test_detect_enum() {
    let source = "<?php\nenum Suit {\n}\n";
    let result = detect_stub_class_kind("Suit", source);
    assert_eq!(
        result,
        Some((ClassLikeKind::Enum, false, false)),
        "should detect an enum"
    );
}

#[test]
fn test_detect_class_in_multi_class_file() {
    // Simulates SPL_c1.php which has many classes and a few interfaces.
    let source = concat!(
        "<?php\n",
        "class SplFileInfo implements Stringable\n{\n}\n",
        "class DirectoryIterator extends SplFileInfo implements SeekableIterator\n{\n}\n",
        "class FilesystemIterator extends DirectoryIterator\n{\n}\n",
        "abstract class SplHeap implements Iterator, Countable\n{\n}\n",
        "interface SplObserver\n{\n}\n",
        "interface SplSubject\n{\n}\n",
        "class SplObjectStorage implements Countable\n{\n}\n",
    );

    assert_eq!(
        detect_stub_class_kind("DirectoryIterator", source),
        Some((ClassLikeKind::Class, false, false)),
        "should find DirectoryIterator as a class in a multi-class file"
    );
    assert_eq!(
        detect_stub_class_kind("SplHeap", source),
        Some((ClassLikeKind::Class, true, false)),
        "should find SplHeap as an abstract class"
    );
    assert_eq!(
        detect_stub_class_kind("SplObserver", source),
        Some((ClassLikeKind::Interface, false, false)),
        "should find SplObserver as an interface"
    );
    assert_eq!(
        detect_stub_class_kind("SplObjectStorage", source),
        Some((ClassLikeKind::Class, false, false)),
        "should find SplObjectStorage as a class"
    );
}

#[test]
fn test_detect_does_not_match_substring() {
    // "Iterator" appears as a substring in "DirectoryIterator" and
    // "FilesystemIterator".  The word boundary check must prevent a
    // false match.
    let source = concat!(
        "<?php\n",
        "interface Iterator\n{\n}\n",
        "class DirectoryIterator extends SplFileInfo\n{\n}\n",
    );

    assert_eq!(
        detect_stub_class_kind("Iterator", source),
        Some((ClassLikeKind::Interface, false, false)),
        "should match the standalone 'Iterator' interface, not the substring in DirectoryIterator"
    );
}

#[test]
fn test_detect_does_not_match_superstring() {
    // Searching for "Directory" should NOT match "DirectoryIterator".
    let source = "<?php\nclass DirectoryIterator extends SplFileInfo\n{\n}\n";
    assert_eq!(
        detect_stub_class_kind("Directory", source),
        None,
        "should not match 'Directory' inside 'DirectoryIterator'"
    );
}

#[test]
fn test_detect_skips_name_in_comments() {
    // The class name appears in a docblock comment, not a declaration.
    let source = concat!(
        "<?php\n",
        "/**\n",
        " * @see DirectoryIterator\n",
        " */\n",
        "class DirectoryIterator extends SplFileInfo\n{\n}\n",
    );
    assert_eq!(
        detect_stub_class_kind("DirectoryIterator", source),
        Some((ClassLikeKind::Class, false, false)),
        "should skip the comment mention and find the actual class declaration"
    );
}

#[test]
fn test_detect_skips_extends_mention() {
    // "SplFileInfo" appears after `extends`, not as a declaration keyword.
    let source = concat!(
        "<?php\n",
        "class DirectoryIterator extends SplFileInfo\n{\n}\n",
    );
    assert_eq!(
        detect_stub_class_kind("SplFileInfo", source),
        None,
        "should not match SplFileInfo in 'extends SplFileInfo' (no declaration keyword before it)"
    );
}

#[test]
fn test_detect_with_fqn_key() {
    // The stub_index key might be a FQN like "Ds\\Set".
    // detect_stub_class_kind should extract the short name "Set".
    let source = concat!(
        "<?php\n",
        "namespace Ds;\n",
        "class Set implements Collection\n{\n}\n",
    );
    assert_eq!(
        detect_stub_class_kind("Ds\\Set", source),
        Some((ClassLikeKind::Class, false, false)),
        "should handle FQN keys by extracting the short name"
    );
}

#[test]
fn test_detect_not_found() {
    let source = "<?php\nclass Foo {\n}\n";
    assert_eq!(
        detect_stub_class_kind("Bar", source),
        None,
        "should return None when the class is not in the source"
    );
}

#[test]
fn test_detect_class_with_extends_and_implements() {
    let source = "<?php\nclass SplFixedArray implements Iterator, ArrayAccess, Countable, IteratorAggregate, JsonSerializable\n{\n}\n";
    assert_eq!(
        detect_stub_class_kind("SplFixedArray", source),
        Some((ClassLikeKind::Class, false, false)),
        "should detect a class with multiple implements"
    );
}

// ── ClassNameContext::matches_kind_flags ─────────────────────────

#[test]
fn test_extends_class_rejects_interface() {
    assert!(
        !ClassNameContext::ExtendsClass.matches_kind_flags(ClassLikeKind::Interface, false, false),
        "ExtendsClass should reject interfaces"
    );
}

#[test]
fn test_extends_class_rejects_final() {
    assert!(
        !ClassNameContext::ExtendsClass.matches_kind_flags(ClassLikeKind::Class, false, true),
        "ExtendsClass should reject final classes"
    );
}

#[test]
fn test_extends_class_accepts_abstract() {
    assert!(
        ClassNameContext::ExtendsClass.matches_kind_flags(ClassLikeKind::Class, true, false),
        "ExtendsClass should accept abstract classes"
    );
}

#[test]
fn test_implements_accepts_interface() {
    assert!(
        ClassNameContext::Implements.matches_kind_flags(ClassLikeKind::Interface, false, false),
        "Implements should accept interfaces"
    );
}

#[test]
fn test_implements_rejects_class() {
    assert!(
        !ClassNameContext::Implements.matches_kind_flags(ClassLikeKind::Class, false, false),
        "Implements should reject classes"
    );
}

#[test]
fn test_trait_use_accepts_trait() {
    assert!(
        ClassNameContext::TraitUse.matches_kind_flags(ClassLikeKind::Trait, false, false),
        "TraitUse should accept traits"
    );
}

#[test]
fn test_trait_use_rejects_class() {
    assert!(
        !ClassNameContext::TraitUse.matches_kind_flags(ClassLikeKind::Class, false, false),
        "TraitUse should reject classes"
    );
}

#[test]
fn test_instanceof_rejects_trait() {
    assert!(
        !ClassNameContext::Instanceof.matches_kind_flags(ClassLikeKind::Trait, false, false),
        "Instanceof should reject traits"
    );
}

#[test]
fn test_instanceof_accepts_enum() {
    assert!(
        ClassNameContext::Instanceof.matches_kind_flags(ClassLikeKind::Enum, false, false),
        "Instanceof should accept enums"
    );
}

#[test]
fn test_new_rejects_abstract() {
    assert!(
        !ClassNameContext::New.matches_kind_flags(ClassLikeKind::Class, true, false),
        "New should reject abstract classes"
    );
}

#[test]
fn test_new_rejects_interface() {
    assert!(
        !ClassNameContext::New.matches_kind_flags(ClassLikeKind::Interface, false, false),
        "New should reject interfaces"
    );
}

// ── ClassNameContext::TypeHint ───────────────────────────────────

#[test]
fn test_type_hint_accepts_class() {
    assert!(
        ClassNameContext::TypeHint.matches_kind_flags(ClassLikeKind::Class, false, false),
        "TypeHint should accept classes"
    );
}

#[test]
fn test_type_hint_accepts_interface() {
    assert!(
        ClassNameContext::TypeHint.matches_kind_flags(ClassLikeKind::Interface, false, false),
        "TypeHint should accept interfaces"
    );
}

#[test]
fn test_type_hint_accepts_enum() {
    assert!(
        ClassNameContext::TypeHint.matches_kind_flags(ClassLikeKind::Enum, false, false),
        "TypeHint should accept enums"
    );
}

#[test]
fn test_type_hint_rejects_trait() {
    assert!(
        !ClassNameContext::TypeHint.matches_kind_flags(ClassLikeKind::Trait, false, false),
        "TypeHint should reject traits"
    );
}

#[test]
fn test_type_hint_accepts_abstract_class() {
    assert!(
        ClassNameContext::TypeHint.matches_kind_flags(ClassLikeKind::Class, true, false),
        "TypeHint should accept abstract classes (they are valid type hints)"
    );
}

#[test]
fn test_type_hint_accepts_final_class() {
    assert!(
        ClassNameContext::TypeHint.matches_kind_flags(ClassLikeKind::Class, false, true),
        "TypeHint should accept final classes"
    );
}

#[test]
fn test_type_hint_is_class_only() {
    assert!(
        ClassNameContext::TypeHint.is_class_only(),
        "TypeHint should be class-only (no constants or functions)"
    );
}

#[test]
fn test_type_hint_matches_class_info() {
    let cls = ClassInfo {
        kind: ClassLikeKind::Class,
        name: crate::atom::atom("Foo"),
        ..Default::default()
    };
    assert!(ClassNameContext::TypeHint.matches(&cls));

    let iface = ClassInfo {
        kind: ClassLikeKind::Interface,
        name: crate::atom::atom("Bar"),
        ..Default::default()
    };
    assert!(ClassNameContext::TypeHint.matches(&iface));

    let enm = ClassInfo {
        kind: ClassLikeKind::Enum,
        name: crate::atom::atom("Baz"),
        ..Default::default()
    };
    assert!(ClassNameContext::TypeHint.matches(&enm));

    let trait_info = ClassInfo {
        kind: ClassLikeKind::Trait,
        name: crate::atom::atom("Qux"),
        ..Default::default()
    };
    assert!(!ClassNameContext::TypeHint.matches(&trait_info));
}

// ── UseImport / UseFunction / UseConst detection ────────────────

#[test]
fn test_detect_use_import_context() {
    let content = "<?php\nuse App";
    let pos = Position {
        line: 1,
        character: 7,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::UseImport,
        "Top-level `use` should produce UseImport"
    );
}

#[test]
fn test_detect_use_function_context() {
    let content = "<?php\nuse function array";
    let pos = Position {
        line: 1,
        character: 19,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::UseFunction,
        "`use function` should produce UseFunction"
    );
}

#[test]
fn test_detect_use_const_context() {
    let content = "<?php\nuse const PHP";
    let pos = Position {
        line: 1,
        character: 14,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::UseConst,
        "`use const` should produce UseConst"
    );
}

#[test]
fn test_detect_use_inside_class_body_is_trait_use() {
    let content = "<?php\nclass Foo {\n    use Some";
    let pos = Position {
        line: 2,
        character: 12,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::TraitUse,
        "`use` inside class body should remain TraitUse"
    );
}

#[test]
fn test_use_import_is_class_only() {
    assert!(
        ClassNameContext::UseImport.is_class_only(),
        "UseImport should be class-only (no constants or functions)"
    );
}

#[test]
fn test_use_function_is_not_class_only() {
    assert!(
        !ClassNameContext::UseFunction.is_class_only(),
        "UseFunction should NOT be class-only (handler shows functions)"
    );
}

#[test]
fn test_use_const_is_not_class_only() {
    assert!(
        !ClassNameContext::UseConst.is_class_only(),
        "UseConst should NOT be class-only (handler shows constants)"
    );
}

#[test]
fn test_use_import_accepts_all_kinds() {
    assert!(ClassNameContext::UseImport.matches_kind_flags(ClassLikeKind::Class, false, false));
    assert!(ClassNameContext::UseImport.matches_kind_flags(ClassLikeKind::Interface, false, false));
    assert!(ClassNameContext::UseImport.matches_kind_flags(ClassLikeKind::Trait, false, false));
    assert!(ClassNameContext::UseImport.matches_kind_flags(ClassLikeKind::Enum, false, false));
}

#[test]
fn test_detect_use_function_with_fqn_partial() {
    let content = "<?php\nuse function App\\Helpers\\format";
    let pos = Position {
        line: 1,
        character: 35,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::UseFunction,
        "`use function` with namespace-qualified partial should produce UseFunction"
    );
}

#[test]
fn test_detect_use_const_with_fqn_partial() {
    let content = "<?php\nuse const App\\Config\\DB";
    let pos = Position {
        line: 1,
        character: 26,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::UseConst,
        "`use const` with namespace-qualified partial should produce UseConst"
    );
}

// ── NamespaceDeclaration detection ──────────────────────────────

#[test]
fn test_detect_namespace_declaration_context() {
    let content = "<?php\nnamespace App";
    let pos = Position {
        line: 1,
        character: 13,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::NamespaceDeclaration,
        "Top-level `namespace` should produce NamespaceDeclaration"
    );
}

#[test]
fn test_detect_namespace_declaration_with_partial_fqn() {
    let content = "<?php\nnamespace App\\Models";
    let pos = Position {
        line: 1,
        character: 22,
    };
    assert_eq!(
        detect_class_name_context(content, pos),
        ClassNameContext::NamespaceDeclaration,
        "`namespace App\\Models` should produce NamespaceDeclaration"
    );
}

#[test]
fn test_namespace_inside_class_body_is_not_declaration() {
    let content = "<?php\nclass Foo {\n    public function bar() {\n        namespace\n";
    let pos = Position {
        line: 3,
        character: 17,
    };
    assert_ne!(
        detect_class_name_context(content, pos),
        ClassNameContext::NamespaceDeclaration,
        "`namespace` inside class body (brace depth >= 1) should not be NamespaceDeclaration"
    );
}

// ── class_edit_texts edge cases ─────────────────────────────────

#[test]
fn test_class_edit_texts_fqn_same_namespace_simplifies() {
    let ns = Some("Demo".to_string());
    let (insert, _filter, use_import) = class_edit_texts("Box", "Demo\\Box", true, true, &ns);
    assert_eq!(insert, "Box", "Insert text should be the relative name");
    assert!(
        use_import.is_none(),
        "No use import needed for same namespace"
    );
}

#[test]
fn test_class_edit_texts_fqn_different_namespace_keeps_fqn() {
    let ns = Some("Demo".to_string());
    let (insert, _filter, use_import) = class_edit_texts("Foo", "Other\\Foo", true, true, &ns);
    assert_eq!(
        insert, "\\Other\\Foo",
        "Insert should have leading backslash"
    );
    assert!(use_import.is_none(), "FQN mode never produces a use import");
}

#[test]
fn test_class_edit_texts_non_fqn_always_short_name() {
    let ns: Option<String> = None;
    let (insert, _filter, use_import) =
        class_edit_texts("Dechunk", "http\\Encoding\\Dechunk", false, false, &ns);
    assert_eq!(
        insert, "Dechunk",
        "Non-FQN mode should insert the short name"
    );
    assert_eq!(
        use_import.as_deref(),
        Some("http\\Encoding\\Dechunk"),
        "Non-FQN mode should import the full FQN"
    );
}

#[test]
fn test_class_edit_texts_fqn_nested_same_namespace() {
    let ns = Some("Demo".to_string());
    let (insert, _filter, use_import) =
        class_edit_texts("Thing", "Demo\\Sub\\Thing", true, true, &ns);
    assert_eq!(
        insert, "Sub\\Thing",
        "Nested same-namespace class should use relative path"
    );
    assert!(use_import.is_none(), "No use import for same namespace");
}

#[test]
fn test_class_edit_texts_leading_backslash_single_segment_same_ns() {
    // Typing `\Demo` (no trailing backslash) in namespace `Demo`.
    // `is_fqn = true` because `has_leading_backslash` is true.
    let ns = Some("Demo".to_string());
    let (insert, _filter, use_import) = class_edit_texts("Box", "Demo\\Box", true, true, &ns);
    assert_eq!(
        insert, "Box",
        "Insert text should be 'Box', not '\\Box' or '\\Demo\\Box'"
    );
    assert!(
        use_import.is_none(),
        "No use import needed for same namespace"
    );
}

#[test]
fn test_class_edit_texts_leading_backslash_single_segment_diff_ns() {
    // Typing `\Other` in namespace `Demo` — different namespace.
    let ns = Some("Demo".to_string());
    let (insert, _filter, use_import) = class_edit_texts("Foo", "Other\\Foo", true, true, &ns);
    assert_eq!(
        insert, "\\Other\\Foo",
        "Insert should have leading backslash for different namespace"
    );
    assert!(use_import.is_none(), "FQN mode never produces a use import");
}

// ── build_affinity_table ────────────────────────────────────────

#[test]
fn test_affinity_table_empty_use_map() {
    let use_map = HashMap::new();
    let ns: Option<String> = None;
    let table = build_affinity_table(&use_map, &ns);
    assert!(
        table.is_empty(),
        "Empty use-map + no namespace → empty table"
    );
}

#[test]
fn test_affinity_table_single_import() {
    let mut use_map = HashMap::new();
    use_map.insert(
        "Brand".to_string(),
        "Luxplus\\Database\\Model\\Brands\\Brand".to_string(),
    );
    let ns: Option<String> = None;
    let table = build_affinity_table(&use_map, &ns);
    assert_eq!(table.get("Luxplus"), Some(&1));
    assert_eq!(table.get("Luxplus\\Database"), Some(&1));
    assert_eq!(table.get("Luxplus\\Database\\Model"), Some(&1));
    assert_eq!(table.get("Luxplus\\Database\\Model\\Brands"), Some(&1));
    assert_eq!(
        table.get("Luxplus\\Database\\Model\\Brands\\Brand"),
        None,
        "Class name itself is not a prefix"
    );
}

#[test]
fn test_affinity_table_file_namespace_only() {
    let use_map = HashMap::new();
    let ns = Some("App\\Http\\Controllers".to_string());
    let table = build_affinity_table(&use_map, &ns);
    assert_eq!(table.get("App"), Some(&1));
    assert_eq!(table.get("App\\Http"), Some(&1));
    assert_eq!(table.get("App\\Http\\Controllers"), Some(&1));
}

#[test]
fn test_affinity_table_file_namespace_plus_imports() {
    let mut use_map = HashMap::new();
    use_map.insert(
        "Request".to_string(),
        "App\\Http\\Requests\\SupplierRequest".to_string(),
    );
    use_map.insert(
        "Brand".to_string(),
        "Luxplus\\Database\\Model\\Brands\\Brand".to_string(),
    );
    let ns = Some("App\\Http\\Controllers".to_string());
    let table = build_affinity_table(&use_map, &ns);
    // "App" appears from file namespace + App\Http\Requests import = 2
    assert_eq!(table.get("App"), Some(&2));
    // "App\\Http" appears from file namespace + App\Http\Requests import = 2
    assert_eq!(table.get("App\\Http"), Some(&2));
    // "App\\Http\\Controllers" from file namespace only
    assert_eq!(table.get("App\\Http\\Controllers"), Some(&1));
    // "App\\Http\\Requests" from import only
    assert_eq!(table.get("App\\Http\\Requests"), Some(&1));
    // "Luxplus" from import only
    assert_eq!(table.get("Luxplus"), Some(&1));
}

#[test]
fn test_affinity_table_global_namespace_import() {
    // A global-namespace import like `use RuntimeException;` has no `\` in the FQN.
    let mut use_map = HashMap::new();
    use_map.insert(
        "RuntimeException".to_string(),
        "RuntimeException".to_string(),
    );
    let ns: Option<String> = None;
    let table = build_affinity_table(&use_map, &ns);
    // No namespace portion → nothing added to the table.
    assert!(
        table.is_empty(),
        "Global-namespace imports contribute nothing to the table"
    );
}

// ── affinity_score ──────────────────────────────────────────────

#[test]
fn test_affinity_score_known_prefix() {
    let mut table = HashMap::new();
    table.insert("Luxplus".to_string(), 11);
    table.insert("Luxplus\\Database".to_string(), 6);
    table.insert("Luxplus\\Database\\Model".to_string(), 6);
    let score = affinity_score("Luxplus\\Database\\Model\\Orders\\Order", &table);
    // Luxplus(11) + Luxplus\Database(6) + Luxplus\Database\Model(6) = 23
    // Luxplus\Database\Model\Orders is not in the table → 0
    assert_eq!(score, 23);
}

#[test]
fn test_affinity_score_no_matching_prefix() {
    let mut table = HashMap::new();
    table.insert("App".to_string(), 4);
    let score = affinity_score("Some\\Random\\Vendor\\Order", &table);
    assert_eq!(score, 0);
}

#[test]
fn test_affinity_score_global_namespace_candidate() {
    // A global-namespace class like "RuntimeException" has no `\` → score 0.
    let mut table = HashMap::new();
    table.insert("App".to_string(), 4);
    let score = affinity_score("RuntimeException", &table);
    assert_eq!(score, 0);
}

#[test]
fn test_affinity_score_empty_table() {
    let table = HashMap::new();
    let score = affinity_score("Luxplus\\Database\\Model\\Orders\\Order", &table);
    assert_eq!(score, 0);
}

// ── match_quality ───────────────────────────────────────────────

#[test]
fn test_match_quality_exact() {
    assert_eq!(match_quality("Order", "Order"), 'a');
}

#[test]
fn test_match_quality_exact_case_insensitive() {
    assert_eq!(match_quality("Order", "order"), 'a');
    assert_eq!(match_quality("ORDER", "order"), 'a');
}

#[test]
fn test_match_quality_starts_with() {
    assert_eq!(match_quality("OrderLine", "Order"), 'b');
    assert_eq!(match_quality("OrderService", "ord"), 'b');
}

#[test]
fn test_match_quality_contains() {
    assert_eq!(match_quality("CheckOrderFlowJob", "Order"), 'c');
    assert_eq!(match_quality("MyOrderService", "order"), 'c');
}

#[test]
fn test_match_quality_empty_prefix_returns_b() {
    assert_eq!(match_quality("Order", ""), 'b');
    assert_eq!(match_quality("AnythingAtAll", ""), 'b');
}

// ── class_sort_text ─────────────────────────────────────────────

#[test]
fn test_class_sort_text_format() {
    let mut table = HashMap::new();
    table.insert("App".to_string(), 4);
    let result = class_sort_text(
        "Order",
        "App\\Models\\Order",
        "Order",
        '2',
        '0',
        false,
        &table,
    );
    // quality='a' (exact), source='2', origin='0', affinity=9999-4=9995 → "9995", demote='0', gap=5-5=0 → "000"
    // source_tier sorts before origin_tier so imported items always rank first.
    assert_eq!(result, "a2099950000_order");
}

#[test]
fn test_class_sort_text_demoted() {
    let table = HashMap::new();
    let normal = class_sort_text(
        "Handler",
        "Vendor\\Handler",
        "Handler",
        '2',
        '0',
        false,
        &table,
    );
    let demoted = class_sort_text(
        "Handler",
        "Vendor\\Handler",
        "Handler",
        '2',
        '0',
        true,
        &table,
    );
    assert!(
        normal < demoted,
        "Demoted should sort after normal: normal={normal}, demoted={demoted}"
    );
}

#[test]
fn test_class_sort_text_quality_beats_tier() {
    let table = HashMap::new();
    // Exact match in tier 2 should beat starts-with match in tier 0.
    let exact_tier2 = class_sort_text("Order", "Vendor\\Order", "Order", '2', '0', false, &table);
    let prefix_tier0 = class_sort_text(
        "OrderLine",
        "Vendor\\OrderLine",
        "Order",
        '0',
        '0',
        false,
        &table,
    );
    assert!(
        exact_tier2 < prefix_tier0,
        "Exact match (tier 2) should sort before prefix match (tier 0): exact={exact_tier2}, prefix={prefix_tier0}"
    );
}

#[test]
fn test_class_sort_text_tier_beats_affinity() {
    let mut table = HashMap::new();
    table.insert("Luxplus".to_string(), 50);
    // Same match quality, but tier 1 should beat tier 2 even with lower affinity.
    let tier1_low = class_sort_text("Order", "App\\Order", "Order", '1', '0', false, &table);
    let tier2_high = class_sort_text("Order", "Luxplus\\Order", "Order", '2', '0', false, &table);
    assert!(
        tier1_low < tier2_high,
        "Tier 1 should sort before tier 2 regardless of affinity: tier1={tier1_low}, tier2={tier2_high}"
    );
}

#[test]
fn test_class_sort_text_affinity_within_same_tier() {
    let mut table = HashMap::new();
    table.insert("Luxplus".to_string(), 11);
    table.insert("Luxplus\\Database".to_string(), 6);
    table.insert("Luxplus\\Database\\Model".to_string(), 6);
    table.insert("App".to_string(), 4);
    // Both tier 2, both exact, but different affinity.
    let high = class_sort_text(
        "Order",
        "Luxplus\\Database\\Model\\Orders\\Order",
        "Order",
        '2',
        '0',
        false,
        &table,
    );
    let low = class_sort_text(
        "Order",
        "App\\Models\\Order",
        "Order",
        '2',
        '0',
        false,
        &table,
    );
    assert!(
        high < low,
        "Higher affinity should sort first: high={high}, low={low}"
    );
}

#[test]
fn test_class_sort_text_demote_after_quality() {
    let table = HashMap::new();
    // A demoted exact match should still beat a non-demoted prefix match.
    let demoted_exact = class_sort_text("Order", "Vendor\\Order", "Order", '2', '0', true, &table);
    let normal_prefix = class_sort_text(
        "OrderLine",
        "Vendor\\OrderLine",
        "Order",
        '2',
        '0',
        false,
        &table,
    );
    assert!(
        demoted_exact < normal_prefix,
        "Demoted exact match should sort before non-demoted prefix match: demoted_exact={demoted_exact}, normal_prefix={normal_prefix}"
    );
}

#[test]
fn test_class_sort_text_alphabetical_tiebreak() {
    let table = HashMap::new();
    // Same quality, tier, affinity, demotion — alphabetical by short name.
    let alpha = class_sort_text("Alpha", "Vendor\\Alpha", "Al", '2', '0', false, &table);
    let beta = class_sort_text("Beta", "Vendor\\Beta", "B", '2', '0', false, &table);
    // Both are starts-with ('b'), tier '2', zero affinity, not demoted.
    // Tiebreak: "alpha" < "beta".
    assert!(
        alpha < beta,
        "Alphabetical tiebreak: alpha={alpha}, beta={beta}"
    );
}

#[test]
fn test_class_sort_text_gap_within_same_affinity() {
    let mut table = HashMap::new();
    // Both classes share the same namespace and thus the same affinity score.
    table.insert("Luxplus".to_string(), 11);
    table.insert("Luxplus\\Core".to_string(), 6);
    table.insert("Luxplus\\Core\\Database".to_string(), 6);
    table.insert("Luxplus\\Core\\Database\\Model".to_string(), 6);
    table.insert("Luxplus\\Core\\Database\\Model\\Products".to_string(), 1);

    // "Product" (len 7, gap 7-3=4) should sort before "ProductFilterTerm" (len 17, gap 17-3=14)
    // when both have the same affinity (same namespace).
    let short = class_sort_text(
        "Product",
        "Luxplus\\Core\\Database\\Model\\Products\\Product",
        "Pro",
        '2',
        '0',
        false,
        &table,
    );
    let long = class_sort_text(
        "ProductFilterTerm",
        "Luxplus\\Core\\Database\\Model\\Products\\Filters\\ProductFilterTerm",
        "Pro",
        '2',
        '0',
        false,
        &table,
    );
    assert!(
        short < long,
        "Shorter name (smaller gap) should sort first within same affinity: short={short}, long={long}"
    );
}

#[test]
fn test_class_sort_text_affinity_beats_gap() {
    let mut table = HashMap::new();
    table.insert("Luxplus".to_string(), 11);
    table.insert("Luxplus\\Database".to_string(), 6);

    // "Proxy" has a tiny gap (5-3=2) but zero affinity.
    // "Product" has a larger gap (7-3=4) but high affinity.
    // Affinity should win because it comes before gap in the sort key.
    let high_affinity = class_sort_text(
        "Product",
        "Luxplus\\Database\\Product",
        "Pro",
        '2',
        '0',
        false,
        &table,
    );
    let low_affinity = class_sort_text("Proxy", "Mockery\\Proxy", "Pro", '2', '0', false, &table);
    assert!(
        high_affinity < low_affinity,
        "Higher affinity should beat smaller gap: high_affinity={high_affinity}, low_affinity={low_affinity}"
    );
}

// ── is_class_declaration_name ───────────────────────────────────

#[test]
fn test_is_class_declaration_class() {
    let content = "<?php\n\nclass T";
    let pos = Position {
        line: 2,
        character: 7,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_no_partial() {
    let content = "<?php\n\nclass ";
    let pos = Position {
        line: 2,
        character: 6,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_interface() {
    let content = "<?php\n\ninterface P";
    let pos = Position {
        line: 2,
        character: 11,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_trait() {
    let content = "<?php\n\ntrait H";
    let pos = Position {
        line: 2,
        character: 7,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_enum() {
    let content = "<?php\n\nenum S";
    let pos = Position {
        line: 2,
        character: 6,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_abstract_class() {
    let content = "<?php\n\nabstract class B";
    let pos = Position {
        line: 2,
        character: 16,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_final_class() {
    let content = "<?php\n\nfinal class S";
    let pos = Position {
        line: 2,
        character: 13,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_readonly_class() {
    let content = "<?php\n\nreadonly class V";
    let pos = Position {
        line: 2,
        character: 16,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_final_readonly_class() {
    let content = "<?php\n\nfinal readonly class M";
    let pos = Position {
        line: 2,
        character: 22,
    };
    assert!(is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_anonymous_class_rejected() {
    let content = "<?php\n\n$x = new class ";
    let pos = Position {
        line: 2,
        character: 16,
    };
    assert!(!is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_extends_rejected() {
    let content = "<?php\n\nclass Foo extends B";
    let pos = Position {
        line: 2,
        character: 21,
    };
    assert!(!is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_implements_rejected() {
    let content = "<?php\n\nclass Foo implements S";
    let pos = Position {
        line: 2,
        character: 25,
    };
    assert!(!is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_new_keyword_rejected() {
    let content = "<?php\n\n$x = new Foo";
    let pos = Position {
        line: 2,
        character: 12,
    };
    assert!(!is_class_declaration_name(content, pos));
}

#[test]
fn test_is_class_declaration_with_namespace() {
    let content = "<?php\n\nnamespace App\\Services;\n\nclass O";
    let pos = Position {
        line: 4,
        character: 7,
    };
    assert!(is_class_declaration_name(content, pos));
}

// ── extract_partial_class_name: open tag suppression ────────────

#[test]
fn test_extract_partial_rejects_open_tag() {
    // Cursor at the end of `<?php` — should NOT offer "php" as a partial.
    let content = "<?php";
    let pos = Position {
        line: 0,
        character: 5,
    };
    assert_eq!(Backend::extract_partial_class_name(content, pos), None);
}

#[test]
fn test_extract_partial_rejects_open_tag_with_trailing_newline() {
    // Cursor at end of the open tag line (before the newline).
    let content = "<?php\n";
    let pos = Position {
        line: 0,
        character: 5,
    };
    assert_eq!(Backend::extract_partial_class_name(content, pos), None);
}

#[test]
fn test_extract_partial_allows_php_prefixed_identifier() {
    // `php_ini_loaded_file` on a normal code line is a valid partial.
    let content = "<?php\nphp_ini";
    let pos = Position {
        line: 1,
        character: 7,
    };
    assert_eq!(
        Backend::extract_partial_class_name(content, pos),
        Some("php_ini".to_string())
    );
}
