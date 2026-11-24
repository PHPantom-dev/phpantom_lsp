//! A class name declared in more than one file must resolve to the same
//! declaration no matter which file was parsed first.
//!
//! Packages ship variant copies of a class behind a `class_exists` guard
//! (Carbon's `DatePeriodBase`, Symfony's polyfilled `RoundingMode`), and
//! the analyzer parses files in parallel, so "whichever parse finished
//! last" made the members such a name resolved to differ from run to run.
//! A declaration PHP always runs beats one behind a guard, the
//! lowest-sorting URI wins among equals, and the declarations that lost
//! are kept so the name survives the winning file dropping it.

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

/// When the winning file stops declaring the name, the other file's
/// declaration takes over rather than the name disappearing.
#[tokio::test]
async fn losing_declaration_takes_over_when_the_winner_drops_it() {
    let backend = create_test_backend();

    backend.update_ast("file:///a_rich.php", RICH);
    backend.update_ast("file:///b_bare.php", BARE);

    // The winner is renamed, so it no longer declares the name.
    backend.update_ast(
        "file:///a_rich.php",
        "<?php\nnamespace Vendor;\nclass Renamed { public function rich(): int { return 1; } }\n",
    );

    let class = backend
        .fqn_class_index()
        .read()
        .get("Vendor\\Variant")
        .cloned()
        .expect("b_bare.php still declares Vendor\\Variant");
    assert!(
        !class.methods.iter().any(|m| m.name == "rich"),
        "the surviving entry should be b_bare.php's declaration"
    );
    assert_eq!(
        backend
            .fqn_uri_index()
            .read()
            .get("Vendor\\Variant")
            .cloned(),
        Some("file:///b_bare.php".to_string()),
        "the URI index should follow the promoted declaration"
    );
}

/// ...and the name goes away only once no file declares it.
#[tokio::test]
async fn name_disappears_once_every_declaration_is_gone() {
    let backend = create_test_backend();

    backend.update_ast("file:///a_rich.php", RICH);
    backend.update_ast("file:///b_bare.php", BARE);

    backend.update_ast("file:///a_rich.php", "<?php\n");
    backend.update_ast("file:///b_bare.php", "<?php\n");

    assert!(
        backend
            .fqn_class_index()
            .read()
            .get("Vendor\\Variant")
            .is_none(),
        "no file declares Vendor\\Variant any more"
    );
    assert!(
        backend
            .fqn_uri_index()
            .read()
            .get("Vendor\\Variant")
            .is_none(),
        "the URI index should not outlive the last declaration"
    );
}

/// Reparsing the losing duplicate on its own must not steal ownership
/// away from the untouched winning file: the previous fix removed a
/// fqn's index entries by name alone before reinserting only the
/// updates in the current batch, so a reparse of just the loser wiped
/// the winner's entry and nothing put it back.
#[tokio::test]
async fn reparsing_losing_file_does_not_steal_ownership() {
    let backend = create_test_backend();

    backend.update_ast("file:///b_bare.php", BARE);
    backend.update_ast("file:///a_rich.php", RICH);

    // Re-parse only the losing file with a trivial edit.
    backend.update_ast(
        "file:///b_bare.php",
        "<?php\nnamespace Vendor;\nclass Variant { /* comment */ }\n",
    );

    let class = backend
        .fqn_class_index()
        .read()
        .get("Vendor\\Variant")
        .cloned()
        .expect("Vendor\\Variant should still be indexed");
    assert!(
        class.methods.iter().any(|m| m.name == "rich"),
        "a_rich.php should still win even though b_bare.php was reparsed"
    );
}

const BASE: &str = "<?php\nnamespace Lib;\nabstract class Base { public static function getList(): array { return []; } }\n";
const REAL_STORE: &str = "<?php\nnamespace Lib;\nclass Store extends Base {}\n";
const DOUBLE_STORE: &str = "<?php\nnamespace Lib;\nclass Store {}\n";
const CALLER: &str = "<?php\n\\Lib\\Store::getList();\n";

fn unknown_members(backend: &phpantom_lsp::Backend, uri: &str, content: &str) -> Vec<String> {
    backend.update_ast(uri, content);
    let mut diags = Vec::new();
    backend.collect_unknown_member_diagnostics(uri, content, &mut diags);
    diags
        .iter()
        .map(|d| crate::common::message_text(&d.message).to_owned())
        .collect()
}

/// A test double of a class is the only declaration of its name until the
/// real file, sorting first, is parsed and takes the name over.  What
/// callers resolved from the double must not outlive that: the real
/// class's inherited members are found from then on.
#[tokio::test]
async fn real_declaration_parsed_after_a_double_brings_its_inherited_members() {
    let backend = create_test_backend();
    backend.update_ast("file:///a_base.php", BASE);
    backend.update_ast("file:///z_double.php", DOUBLE_STORE);

    let before = unknown_members(&backend, "file:///caller.php", CALLER);
    assert_eq!(
        before.len(),
        1,
        "only the double declares Lib\\Store yet, which has no getList(): {before:?}"
    );

    backend.update_ast("file:///b_store.php", REAL_STORE);

    let after = unknown_members(&backend, "file:///caller.php", CALLER);
    assert!(
        after.is_empty(),
        "b_store.php sorts before the double, so getList() comes from its parent: {after:?}"
    );
}

/// The same, when the real declaration arrives with a file loaded on
/// demand for another class it declares.
#[tokio::test]
async fn real_declaration_loaded_on_demand_after_a_double_brings_its_inherited_members() {
    let (backend, dir) = crate::common::create_psr4_workspace(
        r#"{ "autoload": { "psr-4": { "Lib\\": "src/" } } }"#,
        &[
            ("src/Base.php", BASE),
            (
                "src/Other.php",
                "<?php\nnamespace Lib;\nclass Other { public function run(): void {} }\nclass Store extends Base {}\n",
            ),
        ],
    );
    // Sorts after the workspace's own files, wherever the temp dir is.
    backend.update_ast("file:///zzz/double.php", DOUBLE_STORE);

    let before = unknown_members(&backend, "file:///zzz/caller.php", CALLER);
    assert_eq!(before.len(), 1, "only the double is parsed yet: {before:?}");

    let other = "<?php\n(new \\Lib\\Other())->run();\n";
    unknown_members(&backend, "file:///zzz/other_caller.php", other);
    assert!(
        backend.fqn_class_index().read().get("Lib\\Other").is_some(),
        "src/Other.php should have been loaded for Lib\\Other"
    );

    let after = unknown_members(&backend, "file:///zzz/caller.php", CALLER);
    assert!(
        after.is_empty(),
        "src/Other.php sorts before the double and declares Lib\\Store too: {after:?}"
    );
    drop(dir);
}

const GUARDED_WIDGET: &str = "<?php\nnamespace Acme;\nif (!class_exists(Widget::class)) {\n    class Widget { public function old(): void {} }\n}\n";
const REAL_WIDGET: &str = "<?php\nnamespace Acme;\nclass Widget { public function old(): void {} public function render(): string { return ''; } }\n";
const WIDGET_CALLER: &str =
    "<?php\nfunction f(\\Acme\\Widget $w): void { $w->old(); $w->render(); }\n";

/// PHP only runs a declaration behind a `class_exists` guard when the
/// autoloader found no other file for the name, so the unconditional
/// declaration wins even where the guarded file sorts first.
#[tokio::test]
async fn unconditional_declaration_beats_a_guarded_one() {
    for (first, second) in [
        (
            ("file:///a_guard.php", GUARDED_WIDGET),
            ("file:///b_real.php", REAL_WIDGET),
        ),
        (
            ("file:///b_real.php", REAL_WIDGET),
            ("file:///a_guard.php", GUARDED_WIDGET),
        ),
    ] {
        let backend = create_test_backend();
        backend.update_ast(first.0, first.1);
        backend.update_ast(second.0, second.1);

        let uri = backend.fqn_uri_index().read().get("Acme\\Widget").cloned();
        assert_eq!(uri.as_deref(), Some("file:///b_real.php"));
        let unknown = unknown_members(&backend, "file:///caller.php", WIDGET_CALLER);
        assert!(
            unknown.is_empty(),
            "render() is on the real Widget: {unknown:?}"
        );
    }
}

/// A guarded declaration nothing replaces is what PHP runs, so it stays.
#[tokio::test]
async fn guarded_declaration_stands_alone() {
    let backend = create_test_backend();
    backend.update_ast("file:///a_guard.php", GUARDED_WIDGET);

    let unknown = unknown_members(&backend, "file:///caller.php", WIDGET_CALLER);
    assert_eq!(unknown.len(), 1, "only old() is declared: {unknown:?}");
}

/// A guarded copy of a vendor class in the project's `autoload.files`, and
/// the real class under the vendor package's PSR-4 root.  The guarded copy
/// is indexed eagerly; the real class is found through the package scan
/// and loaded once the guarded copy turns out to be what the name
/// resolved to.
#[tokio::test]
async fn guarded_autoload_file_does_not_hide_the_vendor_class() {
    use std::fs;
    use tower_lsp_server::LanguageServer;
    use tower_lsp_server::gen_lsp_types::InitializedParams;

    for guard in ["!class_exists(Widget::class)", "false"] {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let write = |relative: &str, content: &str| {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        };
        write(
            "composer.json",
            r#"{ "autoload": { "psr-4": { "App\\": "src/" }, "files": ["stubs/polyfill.php"] } }"#,
        );
        write(
            "stubs/polyfill.php",
            &format!(
                "<?php\nnamespace Acme;\nif ({guard}) {{\n    class Widget {{ public function old(): void {{}} }}\n}}\n"
            ),
        );
        write("vendor/acme/lib/src/Widget.php", REAL_WIDGET);
        write(
            "vendor/composer/installed.json",
            r#"{ "packages": [ {
                "name": "acme/lib",
                "install-path": "../acme/lib",
                "autoload": { "psr-4": { "Acme\\": "src/" } }
            } ] }"#,
        );
        write(
            "vendor/composer/autoload_files.php",
            "<?php\n$vendorDir = dirname(__DIR__);\n$baseDir = dirname($vendorDir);\nreturn array(\n    'abc' => $baseDir . '/stubs/polyfill.php',\n);\n",
        );

        let backend = create_test_backend();
        *backend.workspace_root().write() = Some(root.to_path_buf());
        backend.initialized(InitializedParams {}).await;

        let caller = crate::common::file_uri(&root.join("src/caller.php"));
        let unknown = unknown_members(&backend, &caller, WIDGET_CALLER);
        assert!(
            unknown.is_empty(),
            "guard `{guard}`: render() is on the vendor Widget: {unknown:?}"
        );
    }
}

const LOSING_VARIANT: &str = "<?php\nnamespace Vendor;\nclass Variant {\n    private int $x = 1;\n    const C = 2;\n    public function f(): int { return $this->x + self::C + static::g() + $this->h() + $this->f(); }\n    public static function g(): int { return 1; }\n    private function h(): int { return 1; }\n}\n";
const VARIANT_CALLER: &str =
    "<?php\nfunction f(\\Vendor\\Variant $v): void { $v->rich(); $v->f(); }\n";

/// Inside the body of a declaration that lost the name to another file,
/// `$this`, `self` and `static` are that declaration, not the winner,
/// while every other file still sees the winner, whichever of the two
/// was resolved first.
#[tokio::test]
async fn this_inside_a_losing_declaration_reads_its_own_members() {
    for caller_first in [false, true] {
        let backend = create_test_backend();
        backend.update_ast("file:///a_rich.php", RICH);
        backend.update_ast("file:///b_variant.php", LOSING_VARIANT);

        let caller = || unknown_members(&backend, "file:///caller.php", VARIANT_CALLER);
        let before = caller_first.then(caller);
        let own = unknown_members(&backend, "file:///b_variant.php", LOSING_VARIANT);
        assert!(
            own.is_empty(),
            "members of the enclosing declaration: {own:?}"
        );
        let after = caller();
        assert_eq!(
            after,
            vec!["Method Vendor\\Variant::f() not found".to_string()],
            "other files see a_rich.php's declaration"
        );
        if let Some(before) = before {
            assert_eq!(before, after);
        }
    }
}
