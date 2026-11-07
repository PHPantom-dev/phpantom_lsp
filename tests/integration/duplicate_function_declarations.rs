//! A function name declared in more than one file must resolve to the
//! same declaration no matter which file was parsed first.
//!
//! Packages ship a native function alongside a `function_exists`-guarded
//! polyfill with a different (looser) signature, and the analyzer parses
//! files in parallel, so "whichever parse finished last" made the
//! signature such a name resolved to differ from run to run. The
//! lowest-sorting URI wins instead, mirroring the class tie-break in
//! `duplicate_class_declarations.rs`.

use crate::common::create_test_backend;

const TYPED: &str = "<?php\nfunction foo(int $i): string { return (string) $i; }\n";
const UNTYPED: &str = "<?php\nfunction foo($i) {}\n";

/// The lowest-sorting URI wins when it is parsed first.
#[tokio::test]
async fn duplicate_declaration_keeps_lowest_uri_parsed_first() {
    let backend = create_test_backend();

    backend.update_ast("file:///a_typed.php", TYPED);
    backend.update_ast("file:///b_untyped.php", UNTYPED);

    let fmap = backend.global_functions().read();
    let (uri, info) = fmap.get("foo").expect("foo should be indexed");
    assert_eq!(uri, "file:///a_typed.php");
    assert!(
        info.return_type.is_some(),
        "a_typed.php sorts first, so its declaration should win"
    );
}

/// ...and still wins when the other file is parsed first.
#[tokio::test]
async fn duplicate_declaration_keeps_lowest_uri_parsed_last() {
    let backend = create_test_backend();

    backend.update_ast("file:///b_untyped.php", UNTYPED);
    backend.update_ast("file:///a_typed.php", TYPED);

    let fmap = backend.global_functions().read();
    let (uri, info) = fmap.get("foo").expect("foo should be indexed");
    assert_eq!(uri, "file:///a_typed.php");
    assert!(
        info.return_type.is_some(),
        "a_typed.php sorts first, so its declaration should win here too"
    );
}

/// Re-parsing the winning file still refreshes its own entry: the
/// tie-break must not freeze an edited file out of its own name.
#[tokio::test]
async fn winning_file_can_still_be_reparsed() {
    let backend = create_test_backend();

    backend.update_ast("file:///b_untyped.php", UNTYPED);
    backend.update_ast("file:///a_typed.php", TYPED);
    backend.update_ast(
        "file:///a_typed.php",
        "<?php\nfunction foo(int $i): int { return $i; }\n",
    );

    let fmap = backend.global_functions().read();
    let (uri, info) = fmap.get("foo").expect("foo should still be indexed");
    assert_eq!(uri, "file:///a_typed.php");
    assert_eq!(
        info.return_type.as_ref().map(|t| t.to_string()),
        Some("int".to_string()),
        "the edit should be reflected"
    );
}

/// When the winning file stops declaring the name, the other file's
/// declaration takes over rather than the name disappearing.
#[tokio::test]
async fn losing_declaration_takes_over_when_the_winner_drops_it() {
    let backend = create_test_backend();

    backend.update_ast("file:///a_typed.php", TYPED);
    backend.update_ast("file:///b_untyped.php", UNTYPED);

    backend.update_ast("file:///a_typed.php", "<?php\n");

    let fmap = backend.global_functions().read();
    let (uri, info) = fmap
        .get("foo")
        .expect("foo should remain from b_untyped.php");
    assert_eq!(uri, "file:///b_untyped.php");
    assert!(
        info.return_type.is_none(),
        "the surviving entry should be b_untyped.php's declaration"
    );
}

/// ...and the name goes away only once no file declares it.
#[tokio::test]
async fn name_disappears_once_every_declaration_is_gone() {
    let backend = create_test_backend();

    backend.update_ast("file:///a_typed.php", TYPED);
    backend.update_ast("file:///b_untyped.php", UNTYPED);

    backend.update_ast("file:///a_typed.php", "<?php\n");
    backend.update_ast("file:///b_untyped.php", "<?php\n");

    assert!(
        backend.global_functions().read().get("foo").is_none(),
        "no file declares foo any more"
    );
}

/// Reparsing the losing duplicate on its own must not steal ownership
/// away from the untouched winning file.
#[tokio::test]
async fn reparsing_losing_file_does_not_steal_ownership() {
    let backend = create_test_backend();

    backend.update_ast("file:///b_untyped.php", UNTYPED);
    backend.update_ast("file:///a_typed.php", TYPED);

    // Re-parse only the losing file with a trivial edit.
    backend.update_ast(
        "file:///b_untyped.php",
        "<?php\nfunction foo($i) { /* comment */ }\n",
    );

    let fmap = backend.global_functions().read();
    let (uri, info) = fmap.get("foo").expect("foo should still be indexed");
    assert_eq!(uri, "file:///a_typed.php");
    assert!(
        info.return_type.is_some(),
        "a_typed.php should still win even though b_untyped.php was reparsed"
    );
}

const WP_NOOPS: &str = "file:///wp/wp-admin/includes/noop.php";
const WP_L10N: &str = "file:///wp/wp-includes/l10n.php";

/// WordPress core declares some of its functions again as no-ops for its
/// script loader. PHP never loads those beside the real ones, so the real
/// declaration wins although the no-ops' file sorts first, and a name only
/// the no-ops declare stays theirs.
#[tokio::test]
async fn wordpress_script_loader_noops_give_way_to_the_real_declaration() {
    for noops_first in [true, false] {
        let backend = create_test_backend();
        let noops = "<?php\nfunction __() {}\nfunction get_file( $path ) {}\n";
        let l10n = "<?php\nfunction __( $text, $domain = 'default' ) { return $text; }\n";
        if noops_first {
            backend.update_ast(WP_NOOPS, noops);
            backend.update_ast(WP_L10N, l10n);
        } else {
            backend.update_ast(WP_L10N, l10n);
            backend.update_ast(WP_NOOPS, noops);
        }

        let fmap = backend.global_functions().read();
        let (uri, info) = fmap.get("__").expect("__ should be indexed");
        assert_eq!(uri, WP_L10N);
        assert_eq!(info.parameters.len(), 2);
        let (uri, _) = fmap.get("get_file").expect("get_file should be indexed");
        assert_eq!(uri, WP_NOOPS);
    }
}

/// The no-ops take the name back only once the real declaration is gone.
#[tokio::test]
async fn wordpress_script_loader_noops_take_over_when_the_real_declaration_goes() {
    let backend = create_test_backend();
    backend.update_ast(WP_NOOPS, "<?php\nfunction __() {}\n");
    backend.update_ast(
        WP_L10N,
        "<?php\nfunction __( $text, $domain = 'default' ) { return $text; }\n",
    );
    backend.update_ast(WP_L10N, "<?php\n");

    let fmap = backend.global_functions().read();
    let (uri, _) = fmap.get("__").expect("__ should remain from noop.php");
    assert_eq!(uri, WP_NOOPS);
}
