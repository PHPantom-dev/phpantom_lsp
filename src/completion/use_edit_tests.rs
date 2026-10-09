use super::*;

/// Backward-compatible helper for tests: returns the position **after**
/// the last existing `use` statement (or the appropriate fallback).
fn find_use_insert_position(content: &str) -> Position {
    let info = analyze_use_block(content);
    if info.existing.is_empty() {
        info.insert_position_for_key("")
    } else {
        let last_line = info.existing.last().expect("non-empty checked above").0;
        Position {
            line: last_line + 1,
            character: 0,
        }
    }
}

// ── use_import_conflicts ────────────────────────────────────────

#[test]
fn conflict_when_short_name_taken_by_different_fqn() {
    let mut use_map = HashMap::new();
    use_map.insert("Exception".to_string(), "Cassandra\\Exception".to_string());

    assert!(use_import_conflicts("App\\Exception", &use_map));
}

#[test]
fn no_conflict_when_same_fqn() {
    let mut use_map = HashMap::new();
    use_map.insert("Exception".to_string(), "App\\Exception".to_string());

    assert!(!use_import_conflicts("App\\Exception", &use_map));
}

#[test]
fn no_conflict_when_different_short_name() {
    let mut use_map = HashMap::new();
    use_map.insert("Exception".to_string(), "Cassandra\\Exception".to_string());

    assert!(!use_import_conflicts("App\\Collection", &use_map));
}

#[test]
fn conflict_is_case_insensitive() {
    let mut use_map = HashMap::new();
    use_map.insert("exception".to_string(), "Cassandra\\Exception".to_string());

    assert!(use_import_conflicts("App\\Exception", &use_map));
}

#[test]
fn no_conflict_when_use_map_empty() {
    let use_map = HashMap::new();

    assert!(!use_import_conflicts("App\\Exception", &use_map));
}

#[test]
fn conflict_with_global_class_fqn() {
    // File has `use Cassandra\Exception;`, importing the global `Exception`
    // (no namespace) should conflict.
    let mut use_map = HashMap::new();
    use_map.insert("Exception".to_string(), "Cassandra\\Exception".to_string());

    assert!(use_import_conflicts("Exception", &use_map));
}

#[test]
fn no_conflict_same_fqn_case_insensitive() {
    let mut use_map = HashMap::new();
    use_map.insert("Exception".to_string(), "app\\exception".to_string());

    assert!(!use_import_conflicts("App\\Exception", &use_map));
}

// ── Leading-segment collision ───────────────────────────────────

#[test]
fn conflict_when_first_segment_matches_alias() {
    // `use Stringable as pq;` — importing `pq\Exception` would be
    // confusing because `pq\Exception` in code resolves through the
    // alias to `Stringable\Exception`.
    let mut use_map = HashMap::new();
    use_map.insert("pq".to_string(), "Stringable".to_string());

    assert!(use_import_conflicts("pq\\Exception", &use_map));
}

#[test]
fn conflict_when_first_segment_matches_alias_case_insensitive() {
    let mut use_map = HashMap::new();
    use_map.insert("PQ".to_string(), "Stringable".to_string());

    assert!(use_import_conflicts("pq\\Exception", &use_map));
}

#[test]
fn no_leading_segment_conflict_for_single_segment_fqn() {
    // `use Stringable;` should not conflict with importing global
    // class `Stringable` — single-segment FQNs skip the leading-
    // segment check to avoid a false positive.
    let mut use_map = HashMap::new();
    use_map.insert("Stringable".to_string(), "Stringable".to_string());

    assert!(!use_import_conflicts("Stringable", &use_map));
}

#[test]
fn leading_segment_conflict_with_deep_namespace() {
    // `use Something as App;` blocks `App\Models\User` because `App`
    // in code would resolve through the alias.
    let mut use_map = HashMap::new();
    use_map.insert("App".to_string(), "Something".to_string());

    assert!(use_import_conflicts("App\\Models\\User", &use_map));
}

#[test]
fn no_leading_segment_conflict_when_no_alias_matches() {
    let mut use_map = HashMap::new();
    use_map.insert("Exception".to_string(), "Cassandra\\Exception".to_string());

    // First segment is `App`, alias is `Exception` — no match.
    assert!(!use_import_conflicts("App\\Collection", &use_map));
}

// ── extract_use_sort_key ────────────────────────────────────────

#[test]
fn sort_key_simple_use() {
    assert_eq!(
        extract_use_sort_key("use Foo\\Bar;"),
        Some("foo\\bar".to_string())
    );
}

#[test]
fn sort_key_with_alias() {
    assert_eq!(
        extract_use_sort_key("use Foo\\Bar as Baz;"),
        Some("foo\\bar".to_string())
    );
}

#[test]
fn sort_key_grouped_use() {
    assert_eq!(
        extract_use_sort_key("use Foo\\{Bar, Baz};"),
        Some("foo\\".to_string())
    );
}

#[test]
fn sort_key_function_use() {
    assert_eq!(
        extract_use_sort_key("use function Foo\\bar;"),
        Some("function foo\\bar".to_string())
    );
}

#[test]
fn sort_key_const_use() {
    assert_eq!(
        extract_use_sort_key("use const Foo\\BAR;"),
        Some("const foo\\bar".to_string())
    );
}

#[test]
fn sort_key_leading_backslash_stripped() {
    assert_eq!(
        extract_use_sort_key("use \\Foo\\Bar;"),
        Some("foo\\bar".to_string())
    );
}

#[test]
fn sort_key_not_a_use_statement() {
    assert_eq!(extract_use_sort_key("class Foo {}"), None);
}

#[test]
fn sort_key_closure_use_ignored() {
    assert_eq!(extract_use_sort_key("use ($var)"), None);
}

// ── analyze_use_block ───────────────────────────────────────────

#[test]
fn collects_existing_uses_with_sort_keys() {
    let content = "<?php\nnamespace App;\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass X {}\n";
    let info = analyze_use_block(content);
    assert_eq!(info.existing.len(), 2);
    assert_eq!(info.existing[0], (2, "foo\\bar".to_string()));
    assert_eq!(info.existing[1], (3, "baz\\qux".to_string()));
}

#[test]
fn fallback_after_namespace_when_no_use() {
    let content = "<?php\nnamespace App;\n\nclass X {}\n";
    let info = analyze_use_block(content);
    assert!(info.existing.is_empty());
    assert_eq!(info.fallback, FirstImport::OwnLine(2));
}

#[test]
fn braced_namespace_import_takes_block_indent() {
    let content = "<?php\nnamespace App {\n    class X {}\n}\n";
    let info = analyze_use_block(content);
    assert_eq!(info.indent, "    ");
    assert_eq!(info.import_text("use Foo;", false), "    use Foo;\n");
}

#[test]
fn flat_namespace_import_has_no_indent() {
    let info = analyze_use_block("<?php\nnamespace App;\n\nclass X {}\n");
    assert_eq!(info.indent, "");
}

#[test]
fn fallback_after_php_open_tag_when_no_namespace() {
    let content = "<?php\n\nclass X {}\n";
    let info = analyze_use_block(content);
    assert!(info.existing.is_empty());
    assert_eq!(info.fallback, FirstImport::OwnLine(1));
}

#[test]
fn trait_use_inside_class_not_collected() {
    let content = "<?php\nnamespace App;\nuse Foo\\Bar;\n\nclass X {\n    use SomeTrait;\n}\n";
    let info = analyze_use_block(content);
    // Only the top-level `use Foo\Bar;` should be collected.
    assert_eq!(info.existing.len(), 1);
    assert_eq!(info.existing[0], (2, "foo\\bar".to_string()));
}

// ── The first import of a block whose declaration shares its line ──

fn inline_at(line: u32, character: u32) -> FirstImport {
    FirstImport::Inline(Position { line, character })
}

#[test]
fn fallback_inside_a_block_that_closes_on_its_declaration_line() {
    let info = analyze_use_block("<?php\nnamespace B { class Foo {} }\n");
    assert!(info.has_namespace);
    assert_eq!(info.fallback, inline_at(1, 13));
}

#[test]
fn fallback_after_a_namespace_statement_with_code_after_it() {
    let info = analyze_use_block("<?php\nnamespace B; class Foo {}\n");
    assert_eq!(info.fallback, inline_at(1, 12));
}

#[test]
fn fallback_after_a_brace_with_the_block_still_open() {
    // The block runs on to later lines, but the code after the brace is
    // on the declaration's line all the same.
    let info = analyze_use_block("<?php\nnamespace B { class Foo {\n}\n}\n");
    assert_eq!(info.fallback, inline_at(1, 13));
}

#[test]
fn fallback_after_a_namespace_that_follows_the_open_tag() {
    let info = analyze_use_block("<?php namespace B { class Foo {} }\n");
    assert!(info.has_namespace);
    assert_eq!(info.fallback, inline_at(0, 19));
}

#[test]
fn fallback_is_the_next_line_for_a_namespace_statement_after_the_open_tag() {
    let info = analyze_use_block("<?php namespace B;\n\nclass Foo {}\n");
    assert!(info.has_namespace);
    assert_eq!(info.fallback, FirstImport::OwnLine(1));
}

#[test]
fn fallback_counts_columns_in_utf16_units() {
    // `Å` is one UTF-16 unit and two UTF-8 bytes.
    let info = analyze_use_block("<?php\nnamespace Å { class Foo {} }\n");
    assert_eq!(info.fallback, inline_at(1, 13));
}

#[test]
fn fallback_stays_on_the_next_line_when_only_a_line_comment_follows() {
    for content in [
        "<?php\nnamespace B; // note\n\nclass Foo {}\n",
        "<?php\nnamespace B { # note\n    class Foo {}\n}\n",
        "<?php\nnamespace B {   \n    class Foo {}\n}\n",
        "<?php\r\nnamespace B;\r\n\r\nclass Foo {}\r\n",
    ] {
        let info = analyze_use_block(content);
        assert_eq!(info.fallback, FirstImport::OwnLine(2), "{content:?}");
    }
}

#[test]
fn fallback_goes_ahead_of_a_block_comment_after_the_declaration() {
    // The next line may be inside the comment.
    let info = analyze_use_block("<?php\nnamespace B { /* note\n   more */ class Foo {} }\n");
    assert_eq!(info.fallback, inline_at(1, 13));
}

#[test]
fn fallback_goes_ahead_of_an_attribute_after_the_declaration() {
    // `#[` opens an attribute, not a comment.
    let info = analyze_use_block("<?php\nnamespace B; #[Attr] class Foo {}\n");
    assert_eq!(info.fallback, inline_at(1, 12));
}

#[test]
fn fallback_is_the_line_after_a_brace_written_on_its_own_line() {
    let info = analyze_use_block("<?php\nnamespace B\n{\n    class Foo {}\n}\n");
    assert_eq!(info.fallback, FirstImport::OwnLine(3));
}

#[test]
fn fallback_ignores_a_comment_between_the_name_and_the_brace() {
    let info = analyze_use_block("<?php\nnamespace B /* ; */ { class Foo {} }\n");
    assert_eq!(info.fallback, inline_at(1, 21));
}

#[test]
fn fallback_after_the_open_tag_when_code_shares_its_line() {
    let info = analyze_use_block("<?php class Foo { public function f(): Helper {} }\n");
    assert!(!info.has_namespace);
    assert_eq!(info.fallback, inline_at(0, 5));
}

#[test]
fn fallback_after_a_declare_when_code_shares_its_line() {
    let info = analyze_use_block("<?php declare(strict_types=1); class Foo {}\n");
    assert_eq!(info.fallback, inline_at(0, 30));
    let info = analyze_use_block("<?php\ndeclare(strict_types=1); class Foo {}\n");
    assert_eq!(info.fallback, inline_at(1, 24));
}

#[test]
fn fallback_is_the_next_line_after_a_declare_that_ends_its_line() {
    let info = analyze_use_block("<?php declare(strict_types=1);\n\nclass Foo {}\n");
    assert_eq!(info.fallback, FirstImport::OwnLine(1));
}

#[test]
fn a_relative_namespace_name_is_not_a_declaration() {
    let info = analyze_use_block("<?php\nnamespace\\Foo\\bar();\n");
    assert!(!info.has_namespace);
    assert_eq!(info.fallback, FirstImport::OwnLine(1));
}

#[test]
fn each_block_of_a_file_written_on_one_line_has_its_own_fallback() {
    let content = "<?php\nnamespace A { class Foo {} } namespace B { class Bar {} }\n";
    let second = content.find("namespace B").expect("the second block");
    let first = analyze_use_block_in(
        content,
        Some((content.find("namespace A").unwrap(), second)),
    );
    let last = analyze_use_block_in(content, Some((second, content.len())));

    assert_eq!(first.fallback, inline_at(1, 13));
    assert_eq!(last.fallback, inline_at(1, 42));
}

// ── UseBlockInfo::insert_position_for ───────────────────────────

#[test]
fn insert_alphabetically_before_first() {
    // Existing: App\Zoo (line 2). Inserting App\Alpha should go before it.
    let info = UseBlockInfo {
        existing: vec![(2, "app\\zoo".to_string())],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert_eq!(
        info.insert_position_for("App\\Alpha"),
        Position {
            line: 2,
            character: 0,
        }
    );
}

#[test]
fn insert_alphabetically_after_last() {
    // Existing: App\Alpha (line 2). Inserting App\Zoo should go after it.
    let info = UseBlockInfo {
        existing: vec![(2, "app\\alpha".to_string())],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert_eq!(
        info.insert_position_for("App\\Zoo"),
        Position {
            line: 3,
            character: 0,
        }
    );
}

#[test]
fn insert_alphabetically_in_the_middle() {
    // Existing: App\Alpha (line 2), App\Zoo (line 3).
    // Inserting App\Middle should go between them.
    let info = UseBlockInfo {
        existing: vec![(2, "app\\alpha".to_string()), (3, "app\\zoo".to_string())],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert_eq!(
        info.insert_position_for("App\\Middle"),
        Position {
            line: 3,
            character: 0,
        }
    );
}

#[test]
fn insert_uses_fallback_when_no_existing() {
    let info = UseBlockInfo {
        existing: vec![],
        fallback: FirstImport::OwnLine(2),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert_eq!(
        info.insert_position_for("App\\Foo"),
        Position {
            line: 2,
            character: 0,
        }
    );
}

#[test]
fn insert_case_insensitive_comparison() {
    // Existing: app\alpha (line 2), app\zoo (line 3).
    // Inserting App\Middle (mixed case) should still land between them.
    let info = UseBlockInfo {
        existing: vec![(2, "app\\alpha".to_string()), (3, "app\\zoo".to_string())],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert_eq!(
        info.insert_position_for("APP\\MIDDLE"),
        Position {
            line: 3,
            character: 0,
        }
    );
}

#[test]
fn insert_among_three_existing() {
    // Existing: A (line 2), C (line 3), E (line 4).
    // Inserting D should go before E (line 4).
    let info = UseBlockInfo {
        existing: vec![
            (2, "a\\a".to_string()),
            (3, "c\\c".to_string()),
            (4, "e\\e".to_string()),
        ],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert_eq!(
        info.insert_position_for("D\\D"),
        Position {
            line: 4,
            character: 0,
        }
    );
}

// ── find_use_insert_position (backward compat) ──────────────────

#[test]
fn compat_insert_after_last_use_statement() {
    let content = "<?php\nnamespace App;\nuse Foo\\Bar;\nuse Baz\\Qux;\n\nclass X {}\n";
    let pos = find_use_insert_position(content);
    assert_eq!(
        pos,
        Position {
            line: 4,
            character: 0
        }
    );
}

#[test]
fn compat_insert_after_namespace_when_no_use() {
    let content = "<?php\nnamespace App;\n\nclass X {}\n";
    let pos = find_use_insert_position(content);
    assert_eq!(
        pos,
        Position {
            line: 2,
            character: 0
        }
    );
}

#[test]
fn compat_insert_after_php_open_tag_when_no_namespace() {
    let content = "<?php\n\nclass X {}\n";
    let pos = find_use_insert_position(content);
    assert_eq!(
        pos,
        Position {
            line: 1,
            character: 0
        }
    );
}

#[test]
fn compat_trait_use_inside_class_not_treated_as_import() {
    let content = "<?php\nnamespace App;\nuse Foo\\Bar;\n\nclass X {\n    use SomeTrait;\n}\n";
    let pos = find_use_insert_position(content);
    // Should insert after `use Foo\Bar;` (line 2), not after `use SomeTrait;`
    assert_eq!(
        pos,
        Position {
            line: 3,
            character: 0
        }
    );
}

// ── build_use_edit (alphabetical) ───────────────────────────────

#[test]
fn build_edit_inserts_at_correct_alpha_position() {
    let info = UseBlockInfo {
        existing: vec![(2, "app\\alpha".to_string()), (3, "app\\zoo".to_string())],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    let edits = build_use_edit("App\\Middle", &info, &Some("App".to_string()))
        .expect("should produce edit");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "use App\\Middle;\n");
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 3,
            character: 0
        }
    );
}

#[test]
fn build_edit_skips_global_class_without_namespace() {
    let info = UseBlockInfo {
        existing: vec![],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert!(build_use_edit("PDO", &info, &None).is_none());
}

#[test]
fn build_edit_includes_global_class_with_namespace() {
    let info = UseBlockInfo {
        existing: vec![],
        fallback: FirstImport::OwnLine(2),
        has_namespace: true,
        indent: String::new(),
        template: None,
    };
    let edits =
        build_use_edit("PDO", &info, &Some("App".to_string())).expect("should produce edit");
    assert_eq!(edits[0].new_text, "\nuse PDO;\n");
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 2,
            character: 0
        }
    );
}

#[test]
fn build_edit_writes_an_import_inline_after_a_declaration_sharing_its_line() {
    let info = analyze_use_block("<?php\nnamespace B { class Foo {} }\n");
    let edits =
        build_use_edit("B\\Helper", &info, &Some("B".to_string())).expect("should produce edit");

    assert_eq!(edits.len(), 1);
    // A space keeps the import off the `{`; the one that followed the `{`
    // already sits between it and `class`.
    assert_eq!(edits[0].new_text, " use B\\Helper;");
    assert_eq!(
        edits[0].range,
        Range {
            start: Position {
                line: 1,
                character: 13
            },
            end: Position {
                line: 1,
                character: 13
            },
        }
    );
}

#[test]
fn build_edit_writes_an_aliased_import_inline() {
    let info = analyze_use_block("<?php\nnamespace B; class Foo {}\n");
    let edits = build_aliased_use_edit("C\\Helper", Some("CHelper"), &info, &Some("B".to_string()))
        .expect("should produce edit");

    assert_eq!(edits[0].new_text, " use C\\Helper as CHelper;");
}

#[test]
fn build_edit_keeps_a_line_of_its_own_when_the_declaration_has_the_line_to_itself() {
    let info = analyze_use_block("<?php\nnamespace B { // note\n    class Foo {}\n}\n");
    let edits =
        build_use_edit("B\\Helper", &info, &Some("B".to_string())).expect("should produce edit");

    assert_eq!(edits[0].new_text, "\nuse B\\Helper;\n");
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 2,
            character: 0
        }
    );
}

#[test]
fn build_edit_does_not_write_inline_beside_existing_imports() {
    // The block has an import, so the new one is placed by it rather than
    // by the declaration.
    let info = analyze_use_block("<?php\nnamespace B {\nuse B\\Alpha;\n    class Foo {}\n}\n");
    let edits =
        build_use_edit("B\\Zeta", &info, &Some("B".to_string())).expect("should produce edit");

    assert_eq!(edits[0].new_text, "use B\\Zeta;\n");
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 3,
            character: 0
        }
    );
}

#[test]
fn build_function_and_const_edits_write_inline_without_a_group_separator() {
    let info = analyze_use_block("<?php\nnamespace B { class Foo {} }\n");

    let function =
        build_use_function_edit("B\\helper", &info).expect("a namespaced function needs an import");
    let constant = build_aliased_typed_use_edit("B\\LIMIT", Some("MAX"), "const", &info)
        .expect("a namespaced constant needs an import");

    assert_eq!(function[0].new_text, " use function B\\helper;");
    assert_eq!(constant[0].new_text, " use const B\\LIMIT as MAX;");
    assert_eq!(function[0].range, constant[0].range);
}

#[test]
fn a_batch_of_inline_imports_keeps_every_import_in_order() {
    let info = analyze_use_block("<?php\nnamespace B { class Foo {} }\n");
    let namespace = Some("B".to_string());

    let mut batch: Vec<TextEdit> = Vec::new();
    for fqn in ["B\\Alpha", "B\\Beta"] {
        let mut edits = build_use_edit(fqn, &info, &namespace).expect("should produce edit");
        if !batch.is_empty() {
            info.drop_repeated_separator(&mut edits);
        }
        batch.extend(edits);
    }

    let texts: Vec<&str> = batch.iter().map(|edit| edit.new_text.as_str()).collect();
    assert_eq!(texts, [" use B\\Alpha;", " use B\\Beta;"]);
}

// ── End-to-end: analyze_use_block + build_use_edit ──────────────

#[test]
fn end_to_end_insert_before_existing_alphabetically() {
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "use Exception;\n",
        "use Stringable;\n",
        "\n",
        "class X {}\n",
    );
    let info = analyze_use_block(content);
    let edits = build_use_edit("Cassandra\\DefaultCluster", &info, &Some("App".to_string()))
        .expect("should produce edit");

    assert_eq!(edits[0].new_text, "use Cassandra\\DefaultCluster;\n");
    // `Cassandra\DefaultCluster` < `Exception`, so insert before line 2.
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 2,
            character: 0,
        }
    );
}

#[test]
fn end_to_end_insert_after_all_existing() {
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Alpha;\n",
        "use App\\Beta;\n",
        "\n",
        "class X {}\n",
    );
    let info = analyze_use_block(content);
    let edits =
        build_use_edit("App\\Zeta", &info, &Some("App".to_string())).expect("should produce edit");

    assert_eq!(edits[0].new_text, "use App\\Zeta;\n");
    // `App\Zeta` > `App\Beta`, so insert after line 3 → line 4.
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 4,
            character: 0,
        }
    );
}

#[test]
fn end_to_end_insert_between_existing() {
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Alpha;\n",
        "use App\\Zeta;\n",
        "\n",
        "class X {}\n",
    );
    let info = analyze_use_block(content);
    let edits = build_use_edit("App\\Middle", &info, &Some("App".to_string()))
        .expect("should produce edit");

    assert_eq!(edits[0].new_text, "use App\\Middle;\n");
    // `App\Middle` > `App\Alpha` but < `App\Zeta`, so insert at line 3.
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 3,
            character: 0,
        }
    );
}

// ── build_use_function_edit ─────────────────────────────────────

#[test]
fn build_function_edit_skips_global_function() {
    let info = UseBlockInfo {
        existing: vec![],
        fallback: FirstImport::OwnLine(1),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    assert!(
        build_use_function_edit("array_map", &info).is_none(),
        "Global function (no backslash) should not produce an import"
    );
}

#[test]
fn build_function_edit_namespaced_no_existing_imports() {
    let info = UseBlockInfo {
        existing: vec![],
        fallback: FirstImport::OwnLine(2),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    let edits = build_use_function_edit("Illuminate\\Support\\enum_value", &info)
        .expect("namespaced function should produce edit");
    // No existing imports → no blank-line separator.
    assert_eq!(
        edits[0].new_text,
        "use function Illuminate\\Support\\enum_value;\n"
    );
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 2,
            character: 0,
        }
    );
}

#[test]
fn build_function_edit_sorts_after_class_imports_with_separator() {
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Models\\User;\n",
        "use Symfony\\Component\\HttpKernel;\n",
        "\n",
    );
    let info = analyze_use_block(content);
    let edits = build_use_function_edit("Illuminate\\Support\\enum_value", &info)
        .expect("should produce edit");

    // The prefixed sort key `"function illuminate\..."` sorts after
    // all bare class import keys, so the function import goes after
    // the last class import (line 3) → insert at line 4.
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 4,
            character: 0,
        },
        "Function import should be placed after all class imports"
    );
    // First function import with existing class imports → blank line separator.
    assert_eq!(
        edits[0].new_text, "\nuse function Illuminate\\Support\\enum_value;\n",
        "Should prepend blank line to separate from class imports"
    );
}

#[test]
fn build_function_edit_no_separator_when_function_group_exists() {
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Models\\User;\n",
        "\n",
        "use function App\\Helpers\\format_price;\n",
        "\n",
    );
    let info = analyze_use_block(content);
    let edits = build_use_function_edit("Illuminate\\Support\\enum_value", &info)
        .expect("should produce edit");

    // There is already a function import → no extra blank line.
    assert_eq!(
        edits[0].new_text, "use function Illuminate\\Support\\enum_value;\n",
        "Should NOT prepend blank line when function group already exists"
    );
    // Alphabetically after `app\helpers\format_price` → after line 4.
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 5,
            character: 0,
        }
    );
}

#[test]
fn build_function_edit_alphabetical_among_existing_functions() {
    let content = concat!(
        "<?php\n",
        "use App\\Models\\User;\n",
        "\n",
        "use function App\\Helpers\\format_price;\n",
        "use function Symfony\\String\\u;\n",
        "\n",
    );
    let info = analyze_use_block(content);
    let edits = build_use_function_edit("Illuminate\\Support\\enum_value", &info)
        .expect("should produce edit");

    // `function illuminate\...` sorts between `function app\...` and
    // `function symfony\...` → insert before line 4 (the Symfony line).
    assert_eq!(
        edits[0].range.start,
        Position {
            line: 4,
            character: 0,
        }
    );
    assert_eq!(
        edits[0].new_text, "use function Illuminate\\Support\\enum_value;\n",
        "No separator needed — already in the function group"
    );
}

#[test]
fn build_function_edit_deeply_namespaced() {
    let info = UseBlockInfo {
        existing: vec![],
        fallback: FirstImport::OwnLine(3),
        has_namespace: false,
        indent: String::new(),
        template: None,
    };
    let edits = build_use_function_edit("Vendor\\Package\\Sub\\Module\\helper_func", &info)
        .expect("deeply namespaced function should produce edit");
    assert_eq!(
        edits[0].new_text,
        "use function Vendor\\Package\\Sub\\Module\\helper_func;\n"
    );
}
