//! A class name declared in more than one file must resolve to the same
//! declaration no matter which file was parsed first.
//!
//! Packages ship variant copies of a class behind a `class_exists` guard
//! (Carbon's `DatePeriodBase`, Symfony's polyfilled `RoundingMode`), and
//! the analyzer parses files in parallel, so "whichever parse finished
//! last" made the members such a name resolved to differ from run to run.
//! The lowest-sorting URI wins instead.

use crate::common::create_test_backend;

const RICH: &str =
    "<?php\nnamespace Vendor;\nclass Variant { public function rich(): int { return 1; } }\n";
const BARE: &str = "<?php\nnamespace Vendor;\nclass Variant {}\n";

/// The lowest-sorting URI wins when it is parsed first.
#[tokio::test]
async fn duplicate_declaration_keeps_lowest_uri_parsed_first() {
    let backend = create_test_backend();

    backend.update_ast("file:///a_rich.php", RICH);
    backend.update_ast("file:///b_bare.php", BARE);

    let class = backend
        .fqn_class_index()
        .read()
        .get("Vendor\\Variant")
        .cloned()
        .expect("Vendor\\Variant should be indexed");
    assert!(
        class.methods.iter().any(|m| m.name == "rich"),
        "a_rich.php sorts first, so its declaration should win"
    );
}

/// ...and still wins when the other file is parsed first.
#[tokio::test]
async fn duplicate_declaration_keeps_lowest_uri_parsed_last() {
    let backend = create_test_backend();

    backend.update_ast("file:///b_bare.php", BARE);
    backend.update_ast("file:///a_rich.php", RICH);

    let class = backend
        .fqn_class_index()
        .read()
        .get("Vendor\\Variant")
        .cloned()
        .expect("Vendor\\Variant should be indexed");
    assert!(
        class.methods.iter().any(|m| m.name == "rich"),
        "a_rich.php sorts first, so its declaration should win here too"
    );
}

/// The URI index agrees with the class index, so go-to-definition lands
/// on the declaration whose members were resolved.
#[tokio::test]
async fn duplicate_declaration_indexes_agree() {
    let backend = create_test_backend();

    backend.update_ast("file:///b_bare.php", BARE);
    backend.update_ast("file:///a_rich.php", RICH);

    let uri = backend
        .fqn_uri_index()
        .read()
        .get("Vendor\\Variant")
        .cloned()
        .expect("Vendor\\Variant should have a URI");
    assert_eq!(uri, "file:///a_rich.php");
}

/// Re-parsing the winning file still refreshes its own entry: the
/// tie-break must not freeze an edited file out of its own name.
#[tokio::test]
async fn winning_file_can_still_be_reparsed() {
    let backend = create_test_backend();

    backend.update_ast("file:///b_bare.php", BARE);
    backend.update_ast("file:///a_rich.php", RICH);
    backend.update_ast(
        "file:///a_rich.php",
        "<?php\nnamespace Vendor;\nclass Variant { public function renamed(): int { return 1; } }\n",
    );

    let class = backend
        .fqn_class_index()
        .read()
        .get("Vendor\\Variant")
        .cloned()
        .expect("Vendor\\Variant should still be indexed");
    assert!(
        class.methods.iter().any(|m| m.name == "renamed"),
        "the edit should be reflected"
    );
    assert!(
        !class.methods.iter().any(|m| m.name == "rich"),
        "the pre-edit member should be gone"
    );
}
