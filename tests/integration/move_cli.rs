//! End-to-end tests for the `move` CLI.
//!
//! Each test builds a throwaway Composer project in a temp directory and
//! drives the whole move through `move_cli::execute`, so what is asserted
//! is the files on disk afterwards and the summary the command reports.

use phpantom_lsp::analyse::OutputFormat;
use phpantom_lsp::move_cli::{MoveOptions, execute};

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("src")).expect("src");
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
    )
    .expect("composer");
    for (relative, content) in files {
        let path = dir.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, content).expect("file");
    }
    dir
}

#[tokio::test]
async fn moves_class_by_fqn_and_updates_references() {
    let dir = project(&[
        (
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        ),
        (
            "src/Consumer.php",
            "<?php\nnamespace App;\n\nuse App\\Old\\Widget;\n\nnew Widget();\n",
        ),
    ]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\Domain\\Gadget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("move");
    assert_eq!(summary.kind, "class");
    assert!(!dir.path().join("src/Old/Widget.php").exists());
    let declaration =
        std::fs::read_to_string(dir.path().join("src/Domain/Gadget.php")).expect("declaration");
    assert!(declaration.contains("namespace App\\Domain;"));
    assert!(declaration.contains("class Gadget"));
    let consumer = std::fs::read_to_string(dir.path().join("src/Consumer.php")).expect("consumer");
    assert!(consumer.contains("use App\\Domain\\Gadget;"));
    assert!(consumer.contains("new Gadget()"));
}

#[tokio::test]
async fn moves_class_by_path() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "src/Old/Widget.php".into(),
        to: "src/New/Widget.php".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");
    let content = std::fs::read_to_string(dir.path().join("src/New/Widget.php")).expect("moved");
    assert!(content.contains("namespace App\\New;"));
}

#[tokio::test]
async fn moves_namespace_by_directory() {
    let dir = project(&[
        (
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        ),
        (
            "src/Old/Nested/Thing.php",
            "<?php\nnamespace App\\Old\\Nested;\n\nclass Thing {}\n",
        ),
        (
            "src/Consumer.php",
            "<?php\nnamespace App;\n\nuse App\\Old\\Widget;\n",
        ),
    ]);
    let options = MoveOptions {
        from: "src/Old".into(),
        to: "src/Domain".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");
    assert!(!dir.path().join("src/Old").exists());
    let widget = std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("widget");
    let nested =
        std::fs::read_to_string(dir.path().join("src/Domain/Nested/Thing.php")).expect("nested");
    assert!(widget.contains("namespace App\\Domain;"));
    assert!(nested.contains("namespace App\\Domain\\Nested;"));
    let consumer = std::fs::read_to_string(dir.path().join("src/Consumer.php")).expect("consumer");
    assert!(consumer.contains("use App\\Domain\\Widget;"));
}

#[tokio::test]
async fn dry_run_changes_nothing() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\New\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: true,
        use_colour: false,
        output_format: OutputFormat::Json,
        global_config: None,
    };

    let summary = execute(&options).await.expect("plan");
    assert!(summary.dry_run);
    assert!(dir.path().join("src/Old/Widget.php").exists());
    assert!(!dir.path().join("src/New/Widget.php").exists());
}

#[tokio::test]
async fn moves_namespace_by_fqn() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old".into(),
        to: "App\\New".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");
    let content = std::fs::read_to_string(dir.path().join("src/New/Widget.php")).expect("moved");
    assert!(content.contains("namespace App\\New;"));
}

#[tokio::test]
async fn warns_when_psr4_cannot_place_the_moved_class() {
    // `Other\` is outside the autoload map, so the declaration is
    // rewritten but the file cannot follow it.  Reporting that as a
    // plain success would hand back a class the autoloader misses.
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "Other\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("move");
    assert_eq!(summary.paths_moved, 0);
    assert!(
        summary.warnings.iter().any(|warning| {
            warning.file.as_deref() == Some("src/Old/Widget.php")
                && warning.message.contains("PSR-4")
        }),
        "expected a PSR-4 warning, got {:?}",
        summary.warnings
    );
    assert!(
        std::fs::read_to_string(dir.path().join("src/Old/Widget.php"))
            .expect("declaration")
            .contains("namespace Other;")
    );
}

#[tokio::test]
async fn a_placed_class_move_warns_about_nothing() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\Domain\\Gadget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("move");
    assert_eq!(summary.paths_moved, 1);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
}

#[tokio::test]
async fn imports_the_siblings_the_moved_class_reached_by_namespace() {
    let dir = project(&[
        (
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget\n{\n    public function make(Cog $cog): Gear\n    {\n        return new Gear($cog);\n    }\n}\n",
        ),
        (
            "src/Old/Cog.php",
            "<?php\nnamespace App\\Old;\n\nclass Cog {}\n",
        ),
        (
            "src/Old/Gear.php",
            "<?php\nnamespace App\\Old;\n\nclass Gear\n{\n    public function __construct(Cog $cog) {}\n}\n",
        ),
    ]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\Domain\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");
    let moved = std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("moved");
    assert!(
        moved.contains("use App\\Old\\Cog;") && moved.contains("use App\\Old\\Gear;"),
        "{moved}"
    );
    // The references keep their short spelling; the imports are what
    // makes them resolve again.
    assert!(moved.contains("make(Cog $cog): Gear"), "{moved}");
}

#[tokio::test]
async fn imports_the_sibling_functions_and_constants_too() {
    let dir = project(&[
        (
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget\n{\n    public function make(): string\n    {\n        return spin(LIMIT);\n    }\n}\n",
        ),
        (
            "src/Old/helpers.php",
            "<?php\nnamespace App\\Old;\n\nconst LIMIT = 3;\n\nfunction spin(int $n): string\n{\n    return (string) $n;\n}\n",
        ),
    ]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\Domain\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");
    let moved = std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("moved");
    assert!(moved.contains("use const App\\Old\\LIMIT;"), "{moved}");
    assert!(moved.contains("use function App\\Old\\spin;"), "{moved}");
}

#[tokio::test]
async fn a_name_the_moved_file_declares_itself_is_not_imported() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget\n{\n    public function make(): Helper\n    {\n        return new Helper();\n    }\n}\n\nclass Helper {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\Domain\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");
    let moved = std::fs::read_to_string(dir.path().join("src/Domain/Widget.php")).expect("moved");
    assert!(!moved.contains("use App\\Old\\Helper;"), "{moved}");
    assert!(!moved.contains("use App\\Old\\Widget;"), "{moved}");
}

#[tokio::test]
async fn warns_when_psr4_cannot_place_the_moved_namespace() {
    // `Other\` is outside the autoload map, so there is nowhere to put
    // the directory.  The declarations are still rewritten, which is
    // what makes the files unreachable and worth reporting.
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old".into(),
        to: "Other\\Domain".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("move");
    assert_eq!(summary.paths_moved, 0);
    assert!(
        summary.warnings.iter().any(|warning| {
            warning.message.contains("Other\\Domain") && warning.message.contains("PSR-4")
        }),
        "expected a PSR-4 warning, got {:?}",
        summary.warnings
    );
    // The file stays put rather than landing in a directory built out
    // of a prefix the destination never had.
    assert!(
        std::fs::read_to_string(dir.path().join("src/Old/Widget.php"))
            .expect("declaration")
            .contains("namespace Other\\Domain;")
    );
}

#[tokio::test]
async fn a_namespace_destination_shorter_than_the_mapping_is_refused_not_a_panic() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old".into(),
        to: "Xy".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: true,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("plan");
    assert_eq!(summary.paths_moved, 0);
    assert!(!summary.warnings.is_empty());
}

#[tokio::test]
async fn moves_a_namespace_between_psr4_mappings() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"src/","Lib\\":"lib/"}}}"#,
    )
    .expect("composer");
    let options = MoveOptions {
        from: "App\\Old".into(),
        to: "Lib\\Domain".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("move");
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert!(!dir.path().join("src/Old").exists());
    assert!(
        std::fs::read_to_string(dir.path().join("lib/Domain/Widget.php"))
            .expect("moved")
            .contains("namespace Lib\\Domain;")
    );
}

/// A project whose `Tests\\` prefix is served by two directories,
/// which is what Composer's array form allows.
fn two_root_project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload-dev":{"psr-4":{"Tests\\":["tests/","shared/tests/"]}}}"#,
    )
    .expect("composer");
    for (relative, content) in files {
        let path = dir.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, content).expect("file");
    }
    dir
}

#[tokio::test]
async fn refuses_a_namespace_that_lives_in_two_psr4_roots() {
    // Naming one root resolves to the namespace both of them serve,
    // and from there the other root is indistinguishable.  Planning
    // the move anyway carries the second root's files along, onto the
    // same destination, so it is refused with both roots named.
    let dir = two_root_project(&[
        (
            "tests/Unit/TokenTransferTest.php",
            "<?php\nnamespace Tests\\Unit;\n\nclass TokenTransferTest {}\n",
        ),
        (
            "shared/tests/Support/Helper.php",
            "<?php\nnamespace Tests\\Support;\n\nclass Helper {}\n",
        ),
    ]);
    let options = MoveOptions {
        from: "shared/tests".into(),
        to: "tests/Shared".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: true,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let error = execute(&options).await.expect_err("refusal");
    assert!(
        error.contains("tests/") && error.contains("shared/tests/"),
        "expected both roots named, got {error}"
    );
    assert!(
        !error.contains("No such file"),
        "expected a refusal, not a derived path failing to open: {error}"
    );
}

#[tokio::test]
async fn a_second_root_that_holds_nothing_does_not_block_the_move() {
    // `shared/tests/` is mapped but was never created, so the moved
    // namespace still sits in exactly one directory.
    let dir = two_root_project(&[(
        "tests/Unit/TokenTransferTest.php",
        "<?php\nnamespace Tests\\Unit;\n\nclass TokenTransferTest {}\n",
    )]);
    let options = MoveOptions {
        from: "Tests\\Unit".into(),
        to: "Tests\\Feature".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("move");
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert!(
        std::fs::read_to_string(dir.path().join("tests/Feature/TokenTransferTest.php"))
            .expect("moved")
            .contains("namespace Tests\\Feature;")
    );
}

#[tokio::test]
async fn two_prefixes_naming_one_directory_are_one_root() {
    // `Tests\\` at `tests/` and `Tests\\Unit\\` at `tests/Unit/` both
    // place `Tests\\Unit` in the same directory, which is one place to
    // move from, not two.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload-dev":{"psr-4":{"Tests\\":"tests/","Tests\\Unit\\":"tests/Unit/"}}}"#,
    )
    .expect("composer");
    let path = dir.path().join("tests/Unit/TokenTransferTest.php");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(
        &path,
        "<?php\nnamespace Tests\\Unit;\n\nclass TokenTransferTest {}\n",
    )
    .expect("file");
    let options = MoveOptions {
        from: "Tests\\Unit".into(),
        to: "Tests\\Feature".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("move");
    assert_eq!(summary.paths_moved, 1);
    assert!(
        std::fs::read_to_string(dir.path().join("tests/Feature/TokenTransferTest.php"))
            .expect("moved")
            .contains("namespace Tests\\Feature;")
    );
}

#[tokio::test]
async fn refuses_occupied_class_destination_without_changes() {
    let old = "<?php\nnamespace App\\Old;\n\nclass Widget {}\n";
    let existing = "<?php\nnamespace App\\New;\n\nclass Widget {}\n";
    let dir = project(&[
        ("src/Old/Widget.php", old),
        ("src/New/Widget.php", existing),
    ]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\New\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let error = execute(&options).await.expect_err("conflict");
    assert!(error.contains("already"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/Old/Widget.php")).expect("old"),
        old
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/New/Widget.php")).expect("existing"),
        existing
    );
}

#[tokio::test]
async fn a_moved_class_keeps_its_imports_indentation_in_a_template() {
    // A `use` inside an `@php` block is indented to the block, and
    // the import is rewritten as a whole statement rather than name
    // by name (an alias can appear or disappear).  Taking the whole
    // line along with it flattened the import against the margin.
    let template = concat!(
        "<div>\n",
        "@php\n",
        "    use App\\Old\\Widget;\n",
        "    $widget = new Widget();\n",
        "@endphp\n",
        "</div>\n",
    );
    let dir = project(&[
        (
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        ),
        ("resources/views/panel.blade.php", template),
    ]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\Domain\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");

    let result = std::fs::read_to_string(dir.path().join("resources/views/panel.blade.php"))
        .expect("template");
    assert!(
        result.contains("    use App\\Domain\\Widget;"),
        "the import has to follow the move, indentation and all:\n{result}"
    );
}

#[tokio::test]
async fn reports_the_old_name_left_behind_in_a_template() {
    // A Blade template names the class as a string the rewriter has
    // no way to resolve, so the move cannot take it along.  Silently
    // omitting it from `files_changed` would read as a complete
    // rewrite.
    let dir = project(&[
        (
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        ),
        (
            "resources/views/panel.blade.php",
            "@php\n$class = 'App\\Old\\Widget';\n@endphp\n",
        ),
    ]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "App\\Domain\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: true,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("plan");
    let residual = summary
        .warnings
        .iter()
        .find(|warning| warning.file.as_deref() == Some("resources/views/panel.blade.php"))
        .unwrap_or_else(|| panic!("expected a template warning, got {:?}", summary.warnings));
    assert_eq!(residual.line, Some(2));
    assert!(residual.message.contains("App\\Old\\Widget"));
}

#[tokio::test]
async fn reports_the_old_directory_left_behind_in_a_path_string() {
    // `app_path()`-relative and project-relative spellings of the
    // moved directory both have to be reported: neither is a symbol
    // reference, and both break the moment the directory moves.
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
    )
    .expect("composer");
    for (relative, content) in [
        (
            "app/Elastic/Config/Mapping.php",
            "<?php\nnamespace App\\Elastic\\Config;\n\nclass Mapping {}\n",
        ),
        (
            "config/audit.php",
            "<?php\nreturn [\n    'ilm' => app_path('Elastic/Config/ILM/'),\n    \
             'map' => base_path('app/Elastic/Config/Mappings.json'),\n];\n",
        ),
    ] {
        let path = dir.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, content).expect("file");
    }
    let options = MoveOptions {
        from: "App\\Elastic\\Config".into(),
        to: "App\\Search\\Config".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: true,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("plan");
    let lines: Vec<Option<usize>> = summary
        .warnings
        .iter()
        .filter(|warning| warning.file.as_deref() == Some("config/audit.php"))
        .map(|warning| warning.line)
        .collect();
    assert_eq!(
        lines,
        vec![Some(3), Some(4)],
        "expected both path spellings, got {:?}",
        summary.warnings
    );
}

#[tokio::test]
async fn a_longer_name_that_merely_starts_the_same_is_not_reported() {
    let dir = project(&[
        (
            "src/Old/Widget.php",
            "<?php\nnamespace App\\Old;\n\nclass Widget {}\n",
        ),
        (
            "src/Older/Gadget.php",
            "<?php\nnamespace App\\Older;\n\nclass Gadget {}\n",
        ),
        (
            "notes.md",
            "The `App\\Older` namespace and the `src/Older` directory stay put.\n",
        ),
    ]);
    let options = MoveOptions {
        from: "App\\Old".into(),
        to: "App\\Domain".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: true,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("plan");
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
}

#[tokio::test]
async fn a_class_leaving_the_global_namespace_is_not_reported_as_left_behind() {
    // The old FQN of a global class is a bare short name, and the
    // move keeps the declaration spelled exactly that way.
    let dir = project(&[
        ("legacy/Widget.php", "<?php\n\nclass Widget {}\n"),
        (
            "src/Consumer.php",
            "<?php\nnamespace App;\n\nnew \\Widget();\n",
        ),
    ]);
    let options = MoveOptions {
        from: "Widget".into(),
        to: "App\\Casts\\Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: true,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    let summary = execute(&options).await.expect("plan");
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
}

#[tokio::test]
async fn a_class_moving_into_the_global_namespace_drops_its_namespace_statement() {
    let dir = project(&[(
        "src/Old/Widget.php",
        "<?php\n\nnamespace App\\Old;\n\nclass Widget {}\n",
    )]);
    let options = MoveOptions {
        from: "App\\Old\\Widget".into(),
        to: "Widget".into(),
        workspace_root: dir.path().to_path_buf(),
        dry_run: false,
        use_colour: false,
        output_format: OutputFormat::Table,
        global_config: None,
    };

    execute(&options).await.expect("move");
    let declaration =
        std::fs::read_to_string(dir.path().join("src/Old/Widget.php")).expect("declaration");
    assert_eq!(declaration, "<?php\n\nclass Widget {}\n");
}
