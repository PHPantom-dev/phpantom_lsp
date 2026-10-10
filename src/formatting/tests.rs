use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use crate::config::FormattingConfig;

use super::external::write_sibling_temp_file;
use super::mago::{format_with_mago, load_mago_format_settings, to_mago_php_version};
use super::{
    FormattingStrategy, Tool, compute_edits, compute_range_edits, execute_strategy,
    resolve_strategy,
};
use tower_lsp::lsp_types::{Position, Range};

// ── compute_edits ───────────────────────────────────────────────

#[test]
fn compute_edits_no_change() {
    let content = "<?php\necho 'hello';\n";
    let edits = compute_edits(content, content);
    assert!(edits.is_empty());
}

#[test]
fn compute_edits_with_change() {
    let original = "<?php\necho 'hello';\n";
    let formatted = "<?php\n\necho 'hello';\n";
    let edits = compute_edits(original, formatted);
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert_eq!(edit.range.start.line, 0);
    assert_eq!(edit.range.start.character, 0);
    assert_eq!(edit.range.end.line, 2);
    assert_eq!(edit.range.end.character, 0);
    assert_eq!(edit.new_text, formatted);
}

#[test]
fn compute_edits_empty_original() {
    let original = "";
    let formatted = "<?php\n";
    let edits = compute_edits(original, formatted);
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert_eq!(edit.range.start.line, 0);
    assert_eq!(edit.range.start.character, 0);
    assert_eq!(edit.range.end.line, 0);
    assert_eq!(edit.range.end.character, 0);
}

#[test]
fn compute_edits_no_trailing_newline() {
    let original = "<?php\necho 'hello';";
    let formatted = "<?php\necho 'hello';\n";
    let edits = compute_edits(original, formatted);
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert_eq!(edit.range.end.line, 1);
    assert_eq!(edit.range.end.character, 13);
}

// ── compute_range_edits ─────────────────────────────────────────

fn lines(start: u32, end: u32) -> Range {
    Range {
        start: Position {
            line: start,
            character: 0,
        },
        end: Position {
            line: end,
            character: 3,
        },
    }
}

fn format_range(original: &str, formatted: &str, range: Range) -> String {
    let edits = compute_range_edits(original, formatted, range);
    crate::text_position::apply_text_edits(original, &edits)
}

#[test]
fn range_edits_no_change() {
    let content = "<?php\necho 'hello';\n";
    assert!(compute_range_edits(content, content, lines(0, 1)).is_empty());
}

#[test]
fn range_edits_keep_changes_outside_the_range() {
    let original = "<?php\nif($a){\nfoo();\n}\nif($b){\nbar();\n}\n";
    let formatted = "<?php\nif ($a) {\n    foo();\n}\nif ($b) {\n    bar();\n}\n";

    assert_eq!(
        format_range(original, formatted, lines(5, 5)),
        "<?php\nif($a){\nfoo();\n}\nif($b){\n    bar();\n}\n"
    );
    assert_eq!(
        format_range(original, formatted, lines(1, 3)),
        "<?php\nif ($a) {\n    foo();\n}\nif($b){\nbar();\n}\n"
    );
}

#[test]
fn range_edits_whole_line_selection_excludes_the_line_it_ends_on() {
    let original = "<?php\nfoo( );\nbar( );\n";
    let formatted = "<?php\nfoo();\nbar();\n";
    let range = Range {
        start: Position {
            line: 1,
            character: 0,
        },
        end: Position {
            line: 2,
            character: 0,
        },
    };

    assert_eq!(
        format_range(original, formatted, range),
        "<?php\nfoo();\nbar( );\n"
    );
}

#[test]
fn range_edits_include_a_hunk_that_straddles_the_range() {
    let original = "<?php\n$a = [\n1,\n2];\n";
    let formatted = "<?php\n$a = [1, 2];\n";

    assert_eq!(format_range(original, formatted, lines(2, 2)), formatted);
}

#[test]
fn range_edits_include_lines_inserted_at_the_edge_of_the_range() {
    let original = "<?php\nfunction a() {}\nfunction b() {}\n";
    let formatted = "<?php\n\nfunction a() {}\n\nfunction b() {}\n";

    assert_eq!(
        format_range(original, formatted, lines(1, 1)),
        "<?php\n\nfunction a() {}\n\nfunction b() {}\n"
    );
    assert_eq!(
        format_range(original, formatted, lines(2, 2)),
        "<?php\nfunction a() {}\n\nfunction b() {}\n"
    );
}

#[test]
fn range_edits_respaced_lines_apply_one_at_a_time() {
    let original = "<?php\nif ($a) {\nfoo();\nbar();\n}\n";
    let formatted = "<?php\nif ($a) {\n    foo();\n    bar();\n}\n";

    assert_eq!(
        format_range(original, formatted, lines(3, 3)),
        "<?php\nif ($a) {\nfoo();\n    bar();\n}\n"
    );
}

#[test]
fn range_edits_moved_lines_go_together() {
    let original = "<?php\nuse B;\nuse C;\nuse A;\n\nfoo( );\n";
    let formatted = "<?php\nuse A;\nuse B;\nuse C;\n\nfoo();\n";

    // The diff inserts `use A;` at the top and deletes it at the bottom.
    assert_eq!(
        format_range(original, formatted, lines(1, 1)),
        "<?php\nuse A;\nuse B;\nuse C;\n\nfoo( );\n"
    );
    assert_eq!(
        format_range(original, formatted, lines(3, 3)),
        "<?php\nuse A;\nuse B;\nuse C;\n\nfoo( );\n"
    );
    assert_eq!(
        format_range(original, formatted, lines(5, 5)),
        "<?php\nuse B;\nuse C;\nuse A;\n\nfoo();\n"
    );
}

#[test]
fn range_edits_last_line_without_trailing_newline() {
    let original = "<?php\nfoo( );\nbar( );";
    let formatted = "<?php\nfoo();\nbar();\n";

    assert_eq!(
        format_range(original, formatted, lines(2, 2)),
        "<?php\nfoo( );\nbar();\n"
    );
}

#[test]
fn range_edits_use_utf16_columns() {
    let original = "<?php\n$s = 'æø'; foo( );";
    let formatted = "<?php\n$s = 'æø';\nfoo();\n";

    let edits = compute_range_edits(original, formatted, lines(1, 1));
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range.end.line, 1);
    assert_eq!(edits[0].range.end.character, 18);
}

// ── resolve_strategy ────────────────────────────────────────────

#[test]
fn strategy_default_config_no_composer_is_builtin() {
    let config = FormattingConfig::default();
    let strategy = resolve_strategy(None, &config, None, None, None);
    assert!(matches!(strategy, FormattingStrategy::BuiltIn(None)));
}

#[test]
fn strategy_builtin_carries_mago_config_when_composer_tools_uninstalled() {
    // laravel/pint is declared in require-dev but its binary is not
    // present in the tempdir's vendor/bin, so the strategy falls back
    // to the built-in formatter — which then picks up the mago.toml.
    let dir = tempfile::tempdir().unwrap();
    let config = FormattingConfig::default();
    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": { "laravel/pint": "^1.0" }
    }))
    .unwrap();
    std::fs::write(
        dir.path().join("mago.toml"),
        "[formatter]\npreset = \"psr-12\"\n",
    )
    .unwrap();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);

    match strategy {
        FormattingStrategy::BuiltIn(path) => {
            assert_eq!(path.unwrap(), dir.path().join("mago.toml"))
        }
        other => panic!("Expected BuiltIn with mago.toml, got {:?}", other),
    }
}

#[test]
fn mago_toml_settings_are_loaded_for_embedded_formatter() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("mago.toml");
    std::fs::write(
        &config_path,
        "[formatter]\npreset = \"psr-12\"\nprint-width = 80\nuse-tabs = true\n",
    )
    .unwrap();

    let settings = load_mago_format_settings(&config_path).unwrap();
    assert_eq!(settings.print_width, 80);
    assert!(settings.use_tabs);
}

#[test]
fn malformed_mago_toml_returns_error_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("mago.toml");
    std::fs::write(&config_path, "this is = not [valid toml").unwrap();

    let result = load_mago_format_settings(&config_path);
    assert!(result.is_err());

    // A malformed config must surface as a formatting error, never a panic.
    let content = "<?php\necho   'hello' ;  \n";
    let result = execute_strategy(
        &FormattingStrategy::BuiltIn(Some(config_path)),
        content,
        &PathBuf::from("/tmp/test.php"),
        None,
        &FormattingConfig::default(),
        crate::types::PhpVersion::default(),
        &AtomicBool::new(false),
    );
    assert!(result.is_err());
}

#[test]
fn strategy_both_disabled() {
    let config = FormattingConfig {
        pint: Some(String::new()),
        php_cs_fixer: Some(String::new()),
        phpcbf: Some(String::new()),
        pint_blade: None,
        blade_php: None,
        timeout: None,
    };
    let strategy = resolve_strategy(None, &config, None, None, None);
    assert!(matches!(strategy, FormattingStrategy::Disabled));
}

#[test]
fn strategy_explicit_commands() {
    let config = FormattingConfig {
        pint: None,
        php_cs_fixer: Some("/usr/bin/php-cs-fixer".to_string()),
        phpcbf: Some("/usr/bin/phpcbf".to_string()),
        pint_blade: None,
        blade_php: None,
        timeout: None,
    };
    let strategy = resolve_strategy(None, &config, None, None, None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 2);
            assert_eq!(tools[0].tool, Tool::PhpCsFixer);
            assert_eq!(tools[0].path, PathBuf::from("/usr/bin/php-cs-fixer"));
            assert_eq!(tools[1].tool, Tool::Phpcbf);
            assert_eq!(tools[1].path, PathBuf::from("/usr/bin/phpcbf"));
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_one_explicit_one_disabled() {
    let config = FormattingConfig {
        pint: None,
        php_cs_fixer: Some("/usr/bin/php-cs-fixer".to_string()),
        phpcbf: Some(String::new()),
        pint_blade: None,
        blade_php: None,
        timeout: None,
    };
    let strategy = resolve_strategy(None, &config, None, None, None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, Tool::PhpCsFixer);
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_require_dev_php_cs_fixer() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let p = vendor_bin.join("php-cs-fixer");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "friendsofphp/php-cs-fixer": "^3.0"
        }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, Tool::PhpCsFixer);
            assert_eq!(tools[0].path, vendor_bin.join("php-cs-fixer"));
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_require_dev_phpcodesniffer() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let p = vendor_bin.join("phpcbf");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "squizlabs/php_codesniffer": "^3.0"
        }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, Tool::Phpcbf);
            assert_eq!(tools[0].path, vendor_bin.join("phpcbf"));
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_phpcs_config_file_without_composer_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let p = vendor_bin.join("phpcbf");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // squizlabs/php_codesniffer is pulled in only transitively (e.g.
    // via slevomat/coding-standard), so it never appears in
    // require-dev directly. A hand-authored phpcs.xml certifies
    // phpcbf on its own, even with no composer.json at all.
    std::fs::write(dir.path().join("phpcs.xml"), "").unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, None, None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, Tool::Phpcbf);
            assert_eq!(tools[0].path, vendor_bin.join("phpcbf"));
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_phpcs_xml_dist_certifies_phpcbf_over_transitive_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let p = vendor_bin.join("phpcbf");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    std::fs::write(dir.path().join("phpcs.xml.dist"), "").unwrap();

    // Only a coding-standard package that pulls in squizlabs/php_codesniffer
    // transitively; the direct dependency check alone would miss this.
    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "slevomat/coding-standard": "^8.0"
        }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, Tool::Phpcbf);
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_mago_formatter_table_keeps_phpcbf_off_a_phpcs_project() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let p = vendor_bin.join("phpcbf");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // The project lints with PHPCS and formats with Mago; the phpcs
    // ruleset must not take the formatter over.
    std::fs::write(dir.path().join("phpcs.xml"), "").unwrap();
    std::fs::write(
        dir.path().join("mago.toml"),
        "[formatter]\nprint-width = 100\n",
    )
    .unwrap();

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": { "squizlabs/php_codesniffer": "^3.0" }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);
    match strategy {
        FormattingStrategy::BuiltIn(path) => {
            assert_eq!(path.unwrap(), dir.path().join("mago.toml"))
        }
        other => panic!("Expected BuiltIn with mago.toml, got {:?}", other),
    }
}

#[test]
fn strategy_mago_toml_without_formatter_table_leaves_phpcbf_in_charge() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let p = vendor_bin.join("phpcbf");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // A mago.toml that only configures the linter says nothing about
    // what the project formats with.
    std::fs::write(dir.path().join("phpcs.xml"), "").unwrap();
    std::fs::write(dir.path().join("mago.toml"), "[linter]\n").unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, None, None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].tool, Tool::Phpcbf);
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_require_dev_both_tools() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    for name in &["php-cs-fixer", "phpcbf"] {
        let p = vendor_bin.join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "friendsofphp/php-cs-fixer": "^3.0",
            "squizlabs/php_codesniffer": "^3.0"
        }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 2);
            assert_eq!(tools[0].tool, Tool::PhpCsFixer);
            assert_eq!(tools[1].tool, Tool::Phpcbf);
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_require_dev_binary_missing_falls_back_to_builtin() {
    // require-dev lists the package but the binary is not installed.
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "friendsofphp/php-cs-fixer": "^3.0"
        }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);
    assert!(
        matches!(strategy, FormattingStrategy::BuiltIn(None)),
        "Expected BuiltIn when binary is missing, got {:?}",
        strategy,
    );
}

#[test]
fn strategy_explicit_overrides_require_dev() {
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();

    let p = vendor_bin.join("php-cs-fixer");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "friendsofphp/php-cs-fixer": "^3.0"
        }
    }))
    .unwrap();

    // User explicitly set a different path.
    let config = FormattingConfig {
        pint: None,
        php_cs_fixer: Some("/opt/php-cs-fixer".to_string()),
        phpcbf: Some(String::new()),
        pint_blade: None,
        blade_php: None,
        timeout: None,
    };
    let strategy = resolve_strategy(Some(dir.path()), &config, None, Some(&composer), None);
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].path, PathBuf::from("/opt/php-cs-fixer"));
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_custom_bin_dir() {
    let dir = tempfile::tempdir().unwrap();
    let custom_bin = dir.path().join("bin");
    std::fs::create_dir_all(&custom_bin).unwrap();

    let p = custom_bin.join("php-cs-fixer");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "friendsofphp/php-cs-fixer": "^3.0"
        }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(
        Some(dir.path()),
        &config,
        None,
        Some(&composer),
        Some("bin"),
    );
    match &strategy {
        FormattingStrategy::External(tools) => {
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].path, custom_bin.join("php-cs-fixer"));
        }
        other => panic!("Expected External, got {:?}", other),
    }
}

#[test]
fn strategy_custom_bin_dir_ignores_default_vendor_bin() {
    // Tools exist in vendor/bin but the project uses a custom bin
    // dir that does NOT contain them — should fall back to built-in.
    let dir = tempfile::tempdir().unwrap();
    let vendor_bin = dir.path().join("vendor/bin");
    std::fs::create_dir_all(&vendor_bin).unwrap();
    let custom_bin = dir.path().join("custom-bin");
    std::fs::create_dir_all(&custom_bin).unwrap();

    let p = vendor_bin.join("php-cs-fixer");
    std::fs::write(&p, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require-dev": {
            "friendsofphp/php-cs-fixer": "^3.0"
        }
    }))
    .unwrap();

    let config = FormattingConfig::default();
    let strategy = resolve_strategy(
        Some(dir.path()),
        &config,
        None,
        Some(&composer),
        Some("custom-bin"),
    );
    // php-cs-fixer is in vendor/bin but NOT in custom-bin.
    assert!(
        matches!(strategy, FormattingStrategy::BuiltIn(None)),
        "Expected BuiltIn when custom bin dir doesn't have the tool, got {:?}",
        strategy,
    );
}

#[test]
fn strategy_no_require_dev_no_config_is_builtin() {
    // composer.json exists but has no require-dev.
    let composer: crate::composer::ComposerPackage = serde_json::from_value(serde_json::json!({
        "require": {
            "php": "^8.0"
        }
    }))
    .unwrap();
    let config = FormattingConfig::default();
    let strategy = resolve_strategy(None, &config, None, Some(&composer), None);
    assert!(matches!(strategy, FormattingStrategy::BuiltIn(None)));
}

// ── format_with_mago ────────────────────────────────────────────

#[test]
fn mago_formats_simple_php() {
    let input = "<?php\necho   'hello' ;  \n";
    let result = format_with_mago(
        input,
        mago_php_version::PHPVersion::PHP84,
        mago_formatter::settings::FormatSettings::default(),
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let formatted = result.unwrap();
    // The formatter should produce valid PHP with normalized spacing.
    assert!(formatted.starts_with("<?php"));
    assert!(formatted.contains("echo"));
}

#[test]
fn mago_returns_error_for_unparseable_php() {
    let input = "<?php\nfunction { broken syntax";
    let result = format_with_mago(
        input,
        mago_php_version::PHPVersion::PHP84,
        mago_formatter::settings::FormatSettings::default(),
    );
    assert!(result.is_err());
}

#[test]
fn mago_preserves_already_formatted() {
    // A well-formatted snippet should round-trip cleanly.
    let input = "<?php\n\necho 'hello';\n";
    let result = format_with_mago(
        input,
        mago_php_version::PHPVersion::PHP84,
        mago_formatter::settings::FormatSettings::default(),
    );
    assert!(result.is_ok());
    let formatted = result.unwrap();
    assert_eq!(formatted, input);
}

#[test]
fn mago_reformats_messy_class() {
    let input = "<?php\n\nnamespace Demo;\nclass User\n{ public function foo(): string\n{\n    return \"1a11a\";}\n}\n";
    let result = format_with_mago(
        input,
        mago_php_version::PHPVersion::PHP84,
        mago_formatter::settings::FormatSettings::default(),
    );
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    let formatted = result.unwrap();
    assert_ne!(
        formatted, input,
        "Formatter should have changed the messy input"
    );
    // The formatter should produce proper brace placement.
    assert!(
        formatted.contains("class User\n{"),
        "Expected class brace on next line, got:\n{}",
        formatted,
    );
}

// ── to_mago_php_version ─────────────────────────────────────────

#[test]
fn php_version_conversion() {
    let v = crate::types::PhpVersion { major: 8, minor: 4 };
    let mago = to_mago_php_version(v);
    assert_eq!(mago.major(), 8);
    assert_eq!(mago.minor(), 4);
    assert_eq!(mago.patch(), 0);
}

// ── sibling temp file ───────────────────────────────────────────

#[test]
fn write_sibling_temp_file_in_same_dir() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("MyClass.php");
    std::fs::write(&original, "<?php\n").unwrap();

    let content = "<?php\necho 'formatted';\n";
    let temp = write_sibling_temp_file(&original, content).unwrap();

    assert_eq!(temp.path().parent(), original.parent());
    let name = temp.path().file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("phpantom-fmt-"));
    assert!(name.ends_with(".php"));

    let read_back = std::fs::read_to_string(temp.path()).unwrap();
    assert_eq!(read_back, content);

    // NamedTempFile auto-deletes on drop — no manual remove needed.
}

// ── execute_strategy with built-in ──────────────────────────────

#[test]
fn execute_builtin_returns_edits_for_unformatted_input() {
    let content = "<?php\necho   'hello' ;  \n";
    let config = FormattingConfig::default();
    let php_version = crate::types::PhpVersion { major: 8, minor: 4 };
    let file_path = PathBuf::from("/tmp/test.php");

    let result = execute_strategy(
        &FormattingStrategy::BuiltIn(None),
        content,
        &file_path,
        None,
        &config,
        php_version,
        &AtomicBool::new(false),
    );
    assert!(result.is_ok());
    let edits = result.unwrap();
    assert!(edits.is_some(), "Expected some edits for unformatted input");
}

#[test]
fn execute_builtin_reformats_messy_class() {
    let content = "<?php\n\nnamespace Demo;\nclass User\n{ public function foo(): string\n{\n    return \"1a11a\";}\n}\n";
    let config = FormattingConfig::default();
    let php_version = crate::types::PhpVersion { major: 8, minor: 4 };
    let file_path = PathBuf::from("/tmp/sandbox.php");

    let result = execute_strategy(
        &FormattingStrategy::BuiltIn(None),
        content,
        &file_path,
        None,
        &config,
        php_version,
        &AtomicBool::new(false),
    );
    assert!(result.is_ok());
    let edits = result.unwrap();
    assert!(edits.is_some(), "Expected edits for messy class, got None");
    let edits = edits.unwrap();
    assert!(!edits.is_empty());
    // The replacement text should have proper PER-CS formatting.
    let new_text = &edits[0].new_text;
    assert!(
        new_text.contains("class User\n{"),
        "Expected class brace on next line, got:\n{}",
        new_text,
    );
}

#[test]
fn execute_disabled_returns_none() {
    let content = "<?php\necho 'hello';\n";
    let config = FormattingConfig {
        pint: None,
        php_cs_fixer: Some(String::new()),
        phpcbf: Some(String::new()),
        pint_blade: None,
        blade_php: None,
        timeout: None,
    };
    let php_version = crate::types::PhpVersion { major: 8, minor: 4 };
    let file_path = PathBuf::from("/tmp/test.php");

    let result = execute_strategy(
        &FormattingStrategy::Disabled,
        content,
        &file_path,
        None,
        &config,
        php_version,
        &AtomicBool::new(false),
    );
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

// ── stdin tool producing empty output ────────────────────────────

/// Writes an executable shell script standing in for a stdin-driven
/// formatter (Pint) and returns its path.
#[cfg(unix)]
fn write_fake_tool(dir: &std::path::Path, name: &str, script_body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{}\n", script_body)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn run_external_pipeline_rejects_empty_stdout_for_nonempty_input() {
    use super::ResolvedTool;
    use super::external::run_external_pipeline;

    let dir = tempfile::tempdir().unwrap();
    // Exits successfully but prints nothing, e.g. a wrapper that formats
    // in place instead of writing the result to stdout.
    let tool_path = write_fake_tool(dir.path(), "fake-pint", "exit 0");
    let file_path = dir.path().join("Example.php");

    let result = run_external_pipeline(
        &[ResolvedTool {
            tool: Tool::Pint,
            path: tool_path,
            standard: None,
        }],
        "<?php\necho 'hello';\n",
        &file_path,
        None,
        &FormattingConfig::default(),
        &AtomicBool::new(false),
    );

    let err = result.expect_err("empty stdout for non-empty input must be rejected");
    assert!(err.contains("pint"), "unexpected error: {err}");
}

#[cfg(unix)]
#[test]
fn phpcbf_fixes_against_the_phpcs_standard() {
    let dir = tempfile::tempdir().unwrap();
    // Rewrites the file it is given (the last argument) with its first
    // argument, so the result shows what standard it was asked to apply.
    let tool_path = write_fake_tool(
        dir.path(),
        "fake-phpcbf",
        "for arg; do file=$arg; done\nprintf '%s\\n' \"$1\" > \"$file\"",
    );
    let config = FormattingConfig {
        php_cs_fixer: Some("/usr/bin/php-cs-fixer".to_string()),
        phpcbf: Some(tool_path.to_string_lossy().into_owned()),
        ..FormattingConfig::default()
    };

    let strategy = resolve_strategy(None, &config, Some("WordPress"), None, None);
    let FormattingStrategy::External(tools) = &strategy else {
        panic!("Expected External, got {:?}", strategy);
    };
    assert_eq!(tools[0].tool, Tool::PhpCsFixer);
    assert_eq!(tools[0].standard, None);

    let phpcbf_only = FormattingStrategy::External(tools[1..].to_vec());
    let formatted = super::format_content(
        &phpcbf_only,
        "<?php\n",
        &dir.path().join("Example.php"),
        None,
        &config,
        crate::types::PhpVersion::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(formatted.as_deref(), Some("--standard=WordPress\n"));
}

#[cfg(unix)]
#[test]
fn run_external_pipeline_allows_empty_stdout_for_empty_input() {
    use super::ResolvedTool;
    use super::external::run_external_pipeline;

    let dir = tempfile::tempdir().unwrap();
    let tool_path = write_fake_tool(dir.path(), "fake-pint", "exit 0");
    let file_path = dir.path().join("Example.php");

    let result = run_external_pipeline(
        &[ResolvedTool {
            tool: Tool::Pint,
            path: tool_path,
            standard: None,
        }],
        "",
        &file_path,
        None,
        &FormattingConfig::default(),
        &AtomicBool::new(false),
    );

    assert_eq!(result.unwrap(), "");
}

#[cfg(unix)]
#[test]
fn run_pint_on_blade_rejects_empty_stdout_for_nonempty_input() {
    use super::ResolvedTool;
    use super::external::run_pint_on_blade;

    let dir = tempfile::tempdir().unwrap();
    let tool_path = write_fake_tool(dir.path(), "fake-pint", "exit 0");
    let file_path = dir.path().join("example.blade.php");

    let result = run_pint_on_blade(
        &ResolvedTool {
            tool: Tool::Pint,
            path: tool_path,
            standard: None,
        },
        "<div>{{ $hello }}</div>\n",
        &file_path,
        None,
        true,
        &FormattingConfig::default(),
        &AtomicBool::new(false),
    );

    let err = result.expect_err("empty stdout for non-empty input must be rejected");
    assert!(err.contains("pint"), "unexpected error: {err}");
}
