//! PHP resolves class, function, and method names case-insensitively.
//! These tests exercise each lookup phase with a casing that
//! differs from the declaration.

use crate::Backend;

static STDCLASS_STUB: &str = "<?php class stdClass {}";
static STRING_FUNCTIONS_STUB: &str = "<?php function strlen(string $string): int {}";

#[test]
fn class_lookup_ignores_case_for_parsed_classes() {
    let backend = Backend::new_test();
    backend.update_ast(
        "file:///user.php",
        "<?php namespace App\\Models; class User {}",
    );

    for name in [
        "App\\Models\\User",
        "app\\models\\user",
        "APP\\MODELS\\USER",
    ] {
        let cls = backend.find_or_load_class(name);
        assert!(cls.is_some(), "lookup for {name:?} should resolve");
        assert_eq!(cls.unwrap().name, "User", "declared spelling is kept");
    }
}

#[test]
fn class_lookup_ignores_case_for_stubs() {
    let mut stubs = std::collections::HashMap::new();
    stubs.insert("stdClass", STDCLASS_STUB);
    let backend = Backend::new_test_with_stubs(stubs);

    for name in ["stdClass", "stdclass", "STDCLASS"] {
        let cls = backend.find_or_load_class(name);
        assert!(cls.is_some(), "lookup for {name:?} should resolve");
        assert_eq!(cls.unwrap().name, "stdClass");
    }
}

#[test]
fn negative_cache_does_not_pin_per_casing() {
    let backend = Backend::new_test();

    // Miss with one casing populates the negative cache…
    assert!(backend.find_or_load_class("app\\foo").is_none());
    assert!(
        backend
            .symbols
            .class_not_found_cache
            .read()
            .contains("APP\\FOO")
    );

    // …but once the class is parsed, every casing resolves again.
    backend.update_ast("file:///foo.php", "<?php namespace App; class Foo {}");
    assert!(backend.find_or_load_class("app\\foo").is_some());
    assert!(backend.find_or_load_class("APP\\FOO").is_some());
}

#[test]
fn function_lookup_ignores_case_for_user_functions() {
    let backend = Backend::new_test();
    backend.update_ast("file:///helpers.php", "<?php function myHelper() {}");

    for name in ["myHelper", "myhelper", "MYHELPER"] {
        let func = backend.find_or_load_function(&[name]);
        assert!(func.is_some(), "lookup for {name:?} should resolve");
        assert_eq!(func.unwrap().name, "myHelper");
    }
}

#[test]
fn function_lookup_ignores_case_for_stubs() {
    let mut function_stubs = std::collections::HashMap::new();
    function_stubs.insert("strlen", STRING_FUNCTIONS_STUB);
    let backend = Backend::new_test_with_all_stubs(
        std::collections::HashMap::new(),
        function_stubs,
        std::collections::HashMap::new(),
    );

    for name in ["strlen", "STRLEN", "StrLen"] {
        let func = backend.find_or_load_function(&[name]);
        assert!(func.is_some(), "lookup for {name:?} should resolve");
        assert_eq!(func.unwrap().name, "strlen");
    }
}

#[test]
fn function_negative_cache_retires_on_declaration() {
    let backend = Backend::new_test();

    // A miss records every candidate spelling, case-insensitively…
    assert!(
        backend
            .find_or_load_function(&["App\\myHelper", "myHelper"])
            .is_none()
    );
    let nf_cache = backend.symbols.function_not_found_cache.read();
    assert!(nf_cache.contains("APP\\MYHELPER"));
    assert!(nf_cache.contains("MYHELPER"));
    drop(nf_cache);

    // …but once a parse declares the function, the lookup resolves
    // instead of answering from the stale negative.
    backend.update_ast(
        "file:///helpers.php",
        "<?php namespace App; function myHelper() {}",
    );
    assert!(
        backend
            .find_or_load_function(&["App\\myHelper", "myHelper"])
            .is_some()
    );
}

#[test]
fn method_lookup_ignores_case() {
    let backend = Backend::new_test();
    backend.update_ast(
        "file:///cls.php",
        "<?php class Widget { public function getValue(): int { return 1; } }",
    );

    let cls = backend.find_or_load_class("Widget").expect("class");
    for name in ["getValue", "getvalue", "GETVALUE"] {
        assert!(cls.has_method(name), "has_method({name:?})");
        assert_eq!(cls.get_method(name).expect("get_method").name, "getValue");
        assert!(
            cls.get_method_arc(name).is_some(),
            "get_method_arc({name:?})"
        );
    }

    // The indexed path (post-`rebuild_method_index`) must agree.
    let mut indexed = crate::types::ClassInfo::clone(&cls);
    indexed.rebuild_method_index();
    assert!(indexed.has_method("GETVALUE"));
    assert_eq!(
        indexed.get_method("getvalue").expect("indexed").name,
        "getValue"
    );
}

/// Re-parsing a lazily-loaded file through `parse_and_cache_content`
/// (the vendor / stub / re-open-after-close path) must evict index
/// entries for classes the file no longer defines.  Previously this
/// path only inserted new FQNs, so a renamed or deleted class kept
/// ghost entries in the FQN indexes and the GTI reverse-inheritance edges
/// (find_implementors / type hierarchy).
#[test]
fn versioned_reparse_evicts_removed_classes() {
    let backend = Backend::new_test();
    let uri = "file:///lib.php";

    backend.parse_and_cache_content(
        "<?php namespace Lib; class Base {} \
         class Child extends Base { public function ghost() {} }",
        uri,
    );

    // First parse populated every index.
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Lib\\Child")
            .is_some(),
        "Child should be in fqn_class_index after first parse"
    );
    assert!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Lib\\Child")
            .is_some(),
        "Child should be in fqn_uri_index after first parse"
    );
    assert!(
        backend
            .symbols
            .gti_index
            .read()
            .values()
            .any(|kids| kids.iter().any(|k| k == "Lib\\Child")),
        "some parent should list Child as an implementor after first parse"
    );

    // Re-parse the same URI with Child (and its method) removed.
    backend.parse_and_cache_content("<?php namespace Lib; class Base {}", uri);

    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Lib\\Child")
            .is_none(),
        "Child must be evicted from fqn_class_index on re-parse"
    );
    assert!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Lib\\Child")
            .is_none(),
        "Child must be evicted from fqn_uri_index on re-parse"
    );
    assert!(
        !backend
            .symbols
            .gti_index
            .read()
            .values()
            .any(|kids| kids.iter().any(|k| k == "Lib\\Child")),
        "no parent should still list the removed Child as an implementor"
    );

    // Base, which the file still defines, must survive the re-parse.
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Lib\\Base")
            .is_some(),
        "Base should survive the re-parse"
    );
}

/// Renaming a class on re-parse repoints the FQN → URI mapping to the
/// new name and drops the old, rather than leaving both.
#[test]
fn versioned_reparse_repoints_renamed_class() {
    let backend = Backend::new_test();
    let uri = "file:///renamed.php";

    backend.parse_and_cache_content("<?php namespace Lib; class OldName {}", uri);
    assert!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Lib\\OldName")
            .is_some()
    );

    backend.parse_and_cache_content("<?php namespace Lib; class NewName {}", uri);
    assert!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Lib\\OldName")
            .is_none(),
        "old name must be evicted after rename"
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Lib\\OldName")
            .is_none(),
        "old ClassInfo must be evicted after rename"
    );
    assert!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Lib\\NewName")
            .is_some(),
        "new name must be indexed after rename"
    );
}

/// The lazily-loaded path settles a class name two files declare the
/// same way the editor path does: on the lowest-sorting URI, with both
/// indexes describing that one file.  It used to keep the first URI it
/// saw but the last `ClassInfo`, so go-to-definition and member
/// resolution could land on different declarations.
#[test]
fn lazy_parse_settles_duplicate_on_the_lowest_uri() {
    let backend = Backend::new_test();

    backend.parse_and_cache_content(
        "<?php namespace Vendor; class Variant {}",
        "file:///b_bare.php",
    );
    backend.parse_and_cache_content(
        "<?php namespace Vendor; class Variant { public function rich() {} }",
        "file:///a_rich.php",
    );

    assert_eq!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Vendor\\Variant")
            .cloned(),
        Some("file:///a_rich.php".to_string()),
        "the lowest-sorting URI should win"
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Vendor\\Variant")
            .is_some_and(|cls| cls.methods.iter().any(|m| m.name == "rich")),
        "the class index must describe the file the URI index points at"
    );
}

/// ...and when that file stops declaring the name, the other file's
/// declaration takes over instead of the name becoming unresolvable.
#[test]
fn lazy_parse_promotes_the_runner_up_when_the_winner_drops_the_name() {
    let backend = Backend::new_test();

    backend.parse_and_cache_content(
        "<?php namespace Vendor; class Variant { public function rich() {} }",
        "file:///a_rich.php",
    );
    backend.parse_and_cache_content(
        "<?php namespace Vendor; class Variant { public function bare() {} }",
        "file:///b_bare.php",
    );

    // The winner is re-parsed without the class (renamed on disk).
    backend.parse_and_cache_content(
        "<?php namespace Vendor; class Renamed {}",
        "file:///a_rich.php",
    );

    assert_eq!(
        backend
            .symbols
            .fqn_uri_index
            .read()
            .get("Vendor\\Variant")
            .cloned(),
        Some("file:///b_bare.php".to_string()),
        "b_bare.php still declares the name"
    );
    assert!(
        backend
            .symbols
            .fqn_class_index
            .read()
            .get("Vendor\\Variant")
            .is_some_and(|cls| cls.methods.iter().any(|m| m.name == "bare")),
        "the promoted declaration's members must be the ones indexed"
    );
}
