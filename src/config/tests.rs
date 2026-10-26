use super::*;
use std::io::Write;

/// Load a project config with the global layer switched off, so none
/// of the tests below picks up the real
/// `~/.config/phpantom_lsp/.phpantom.toml` of the machine running
/// them.  The global layer is covered by the tests that call
/// [`load_config_from`] with a temp path instead.
fn load_config(workspace_root: &Path) -> Result<Config, ConfigError> {
    load_config_from(workspace_root, None)
}

#[test]
fn create_default_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let result = create_default_config(dir.path()).unwrap();
    assert!(result, "should report that the file was created");
    let path = dir.path().join(CONFIG_FILE_NAME);
    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("#:schema"));
}

#[test]
fn create_default_does_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "# custom\n").unwrap();
    let result = create_default_config(dir.path()).unwrap();
    assert!(!result, "should report that the file already exists");
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        content, "# custom\n",
        "existing file must not be overwritten"
    );
}

#[test]
fn default_content_parses_successfully() {
    let config: Config = toml::from_str(DEFAULT_CONFIG_CONTENT).unwrap();
    assert!(config.php.version.is_none());
    assert!(!config.diagnostics.unresolved_member_access_enabled());
    assert!(!config.diagnostics.extra_arguments_enabled());
    assert!(!config.diagnostics.report_magic_properties_enabled());
    assert!(config.diagnostics.ignore.is_empty());
    assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
    assert_eq!(
        config.semantic_tokens.mode(),
        SemanticTokensMode::Contextual
    );
    assert!(config.formatting.php_cs_fixer.is_none());
    assert!(config.formatting.phpcbf.is_none());
    assert!(config.formatting.timeout.is_none());
    assert_eq!(config.formatting.timeout_ms(), 10_000);
    assert!(config.phpstan.command.is_none());
    assert!(config.phpstan.memory_limit.is_none());
    assert!(config.phpstan.timeout.is_none());
    assert_eq!(config.phpstan.timeout_ms(), 60_000);
    assert!(config.phpcs.command.is_none());
    assert!(config.phpcs.standard.is_none());
    assert!(config.phpcs.timeout.is_none());
    assert_eq!(config.phpcs.timeout_ms(), 30_000);
    assert!(config.phpmd.command.is_none());
    assert!(config.phpmd.ruleset.is_none());
    assert_eq!(config.phpmd.timeout_ms(), 30_000);
    assert!(!config.phpmd.is_disabled());
    assert!(config.mago.command.is_none());
    // Unset means "follow mago.toml", not on or off.
    assert!(config.mago.lint.is_none());
    assert!(config.mago.analyze.is_none());
    assert!(config.mago.lint_timeout.is_none());
    assert!(config.mago.analyze_timeout.is_none());
    assert_eq!(config.mago.lint_timeout_ms(), 30_000);
    assert_eq!(config.mago.analyze_timeout_ms(), 60_000);
    assert!(!config.mago.is_disabled());
}

#[test]
fn missing_file_returns_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.php.version.is_none());
    assert!(!config.diagnostics.unresolved_member_access_enabled());
    assert!(!config.diagnostics.extra_arguments_enabled());
    assert!(!config.diagnostics.report_magic_properties_enabled());
    assert!(config.diagnostics.ignore.is_empty());
    assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
    assert_eq!(
        config.semantic_tokens.mode(),
        SemanticTokensMode::Contextual
    );
    assert!(config.formatting.php_cs_fixer.is_none());
    assert!(config.formatting.phpcbf.is_none());
    assert!(config.phpstan.command.is_none());
    assert!(config.phpcs.command.is_none());
    assert!(config.mago.command.is_none());
}

#[test]
fn empty_file_returns_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.php.version.is_none());
    assert!(!config.diagnostics.unresolved_member_access_enabled());
    assert!(!config.diagnostics.extra_arguments_enabled());
    assert!(!config.diagnostics.report_magic_properties_enabled());
    assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
    assert_eq!(
        config.semantic_tokens.mode(),
        SemanticTokensMode::Contextual
    );
    assert!(config.formatting.php_cs_fixer.is_none());
    assert!(config.formatting.phpcbf.is_none());
    assert!(config.phpstan.command.is_none());
    assert!(config.phpcs.command.is_none());
    assert!(config.mago.command.is_none());
}

#[test]
fn parses_php_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[php]\nversion = \"8.3\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.php.version.as_deref(), Some("8.3"));
}

#[test]
fn parses_diagnostics_section() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\nunresolved-member-access = true\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.unresolved_member_access_enabled());
}

#[test]
fn unresolved_member_access_defaults_to_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(!config.diagnostics.unresolved_member_access_enabled());
}

#[test]
fn parses_report_magic_properties() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\nreport-magic-properties = true\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.report_magic_properties_enabled());
}

#[test]
fn report_magic_properties_defaults_to_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(!config.diagnostics.report_magic_properties_enabled());
}

#[test]
fn parses_partial_union_members() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\npartial-union-members = true\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.partial_union_members_enabled());
}

#[test]
fn partial_union_members_defaults_to_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(!config.diagnostics.partial_union_members_enabled());
}

#[test]
fn parses_downgrade_nullable_argument_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        "[diagnostics]\ndowngrade-nullable-argument-mismatch = true\n",
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(
        config
            .diagnostics
            .downgrade_nullable_argument_mismatch_enabled()
    );
}

#[test]
fn downgrade_nullable_argument_mismatch_defaults_to_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(
        !config
            .diagnostics
            .downgrade_nullable_argument_mismatch_enabled()
    );
}

#[test]
fn extra_arguments_defaults_to_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(!config.diagnostics.extra_arguments_enabled());
}

#[test]
fn parses_extra_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\nextra-arguments = true\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.extra_arguments_enabled());
}

#[test]
fn diagnostics_ignore_defaults_to_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.ignore.is_empty());
}

#[test]
fn parses_diagnostics_ignore_rule() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        r#"
[[diagnostics.ignore]]
message = "^Call to deprecated function some_legacy_helper\\(\\)"
path = "tests/**"
identifier = "deprecated_usage"
"#,
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.diagnostics.ignore.len(), 1);
    let rule = &config.diagnostics.ignore[0];
    assert_eq!(
        rule.message.as_deref(),
        Some("^Call to deprecated function some_legacy_helper\\(\\)")
    );
    assert_eq!(rule.path.as_deref(), Some("tests/**"));
    assert_eq!(rule.identifier.as_deref(), Some("deprecated_usage"));
}

#[test]
fn parses_multiple_diagnostics_ignore_rules_with_partial_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        r#"
[[diagnostics.ignore]]
path = "vendor/**"

[[diagnostics.ignore]]
identifier = "unused_variable"
"#,
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.diagnostics.ignore.len(), 2);
    assert_eq!(
        config.diagnostics.ignore[0].path.as_deref(),
        Some("vendor/**")
    );
    assert!(config.diagnostics.ignore[0].message.is_none());
    assert!(config.diagnostics.ignore[0].identifier.is_none());
    assert_eq!(
        config.diagnostics.ignore[1].identifier.as_deref(),
        Some("unused_variable")
    );
    assert!(config.diagnostics.ignore[1].path.is_none());
}

#[test]
fn invalid_toml_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[diagnostics\nbroken").unwrap();
    let result = load_config(dir.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("failed to parse"));
}

#[test]
fn unknown_keys_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "[diagnostics]").unwrap();
    writeln!(f, "unresolved-member-access = true").unwrap();
    writeln!(f, "some-future-tool = false").unwrap();
    drop(f);
    // Unknown keys should NOT cause a parse error — forward compatibility.
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.unresolved_member_access_enabled());
}

#[test]
fn unknown_sections_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        "[php]\nversion = \"8.4\"\n\n[some-future-section]\nkey = \"value\"\n",
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.php.version.as_deref(), Some("8.4"));
}

#[test]
fn parses_phpstan_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpstan]\ncommand = \"/usr/bin/phpstan\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpstan.command.as_deref(), Some("/usr/bin/phpstan"));
}

#[test]
fn parses_phpstan_memory_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpstan]\nmemory-limit = \"2G\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpstan.memory_limit.as_deref(), Some("2G"));
}

#[test]
fn parses_phpstan_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpstan]\ntimeout = 30000\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpstan.timeout_ms(), 30_000);
}

#[test]
fn phpstan_empty_string_disables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpstan]\ncommand = \"\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpstan.command.as_deref(), Some(""));
    assert!(config.phpstan.is_disabled());
}

#[test]
fn phpstan_defaults() {
    let config = Config::default();
    assert!(config.phpstan.command.is_none());
    assert!(config.phpstan.memory_limit.is_none());
    assert!(config.phpstan.timeout.is_none());
    assert_eq!(config.phpstan.timeout_ms(), 60_000);
    assert!(!config.phpstan.is_disabled());
}

#[test]
fn full_example_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        r#"
[php]
version = "8.2"

[diagnostics]
unresolved-member-access = true
extra-arguments = true
report-magic-properties = true

[[diagnostics.ignore]]
path = "tests/**"

[[diagnostics.ignore]]
identifier = "deprecated_usage"
message = "^Call to deprecated function some_legacy_helper\\(\\)"

[indexing]
strategy = "self"
exclude = ["generated", "web/sites/default/files"]
extensions = ["module", "inc"]

[semantic_tokens]
mode = "full"

[formatting]
php-cs-fixer = ""
phpcbf = "/usr/local/bin/phpcbf"
timeout = 5000

[phpstan]
command = "/usr/local/bin/phpstan"
memory-limit = "2G"
timeout = 30000

[phpcs]
command = "/usr/local/bin/phpcs"
standard = "PSR12"
timeout = 15000

[phpmd]
command = ""
ruleset = "phpmd.xml"
timeout = 12000

[mago]
command = "/usr/local/bin/mago"
lint = true
analyze = false
lint-timeout = 15000
analyze-timeout = 45000
"#,
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.php.version.as_deref(), Some("8.2"));
    assert!(config.diagnostics.unresolved_member_access_enabled());
    assert!(config.diagnostics.extra_arguments_enabled());
    assert!(config.diagnostics.report_magic_properties_enabled());
    assert_eq!(config.diagnostics.ignore.len(), 2);
    assert_eq!(
        config.diagnostics.ignore[0].path.as_deref(),
        Some("tests/**")
    );
    assert_eq!(
        config.diagnostics.ignore[1].identifier.as_deref(),
        Some("deprecated_usage")
    );
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::SelfScan));
    assert_eq!(
        config.indexing.exclude(),
        &[
            "generated".to_string(),
            "web/sites/default/files".to_string()
        ]
    );
    assert_eq!(
        config.indexing.extensions(),
        &["module".to_string(), "inc".to_string()]
    );
    assert_eq!(config.semantic_tokens.mode, Some(SemanticTokensMode::Full));
    assert_eq!(config.formatting.php_cs_fixer.as_deref(), Some(""));
    assert_eq!(
        config.formatting.phpcbf.as_deref(),
        Some("/usr/local/bin/phpcbf")
    );
    assert_eq!(config.formatting.timeout_ms(), 5000);
    assert_eq!(
        config.phpstan.command.as_deref(),
        Some("/usr/local/bin/phpstan")
    );
    assert_eq!(config.phpstan.memory_limit.as_deref(), Some("2G"));
    assert_eq!(config.phpstan.timeout_ms(), 30_000);
    assert_eq!(
        config.phpcs.command.as_deref(),
        Some("/usr/local/bin/phpcs")
    );
    assert_eq!(config.phpcs.standard.as_deref(), Some("PSR12"));
    assert_eq!(config.phpcs.timeout_ms(), 15_000);
    assert!(config.phpmd.is_disabled());
    assert_eq!(config.phpmd.ruleset.as_deref(), Some("phpmd.xml"));
    assert_eq!(config.phpmd.timeout_ms(), 12_000);
    assert_eq!(config.mago.command.as_deref(), Some("/usr/local/bin/mago"));
    assert_eq!(config.mago.lint, Some(true));
    assert_eq!(config.mago.analyze, Some(false));
    assert_eq!(config.mago.lint_timeout_ms(), 15_000);
    assert_eq!(config.mago.analyze_timeout_ms(), 45_000);
    assert!(!config.mago.is_disabled());
}

#[test]
fn parses_indexing_strategy_composer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[indexing]\nstrategy = \"composer\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::Composer));
}

#[test]
fn parses_semantic_tokens_mode_contextual() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[semantic_tokens]\nmode = \"contextual\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(
        config.semantic_tokens.mode(),
        SemanticTokensMode::Contextual
    );
}

#[test]
fn parses_semantic_tokens_mode_full() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[semantic_tokens]\nmode = \"full\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.semantic_tokens.mode(), SemanticTokensMode::Full);
}

#[test]
fn parses_semantic_tokens_mode_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[semantic_tokens]\nmode = \"off\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.semantic_tokens.mode(), SemanticTokensMode::Off);
}

#[test]
fn invalid_semantic_tokens_mode_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[semantic_tokens]\nmode = \"bogus\"\n").unwrap();
    let result = load_config(dir.path());
    assert!(result.is_err());
}

#[test]
fn parses_laravel_schema_options() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        r#"
[laravel.schema]
enabled = false
paths = ["database/schema", "extra/schema.sql"]
"#,
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(!config.laravel.schema.enabled());
    assert_eq!(
        config.laravel.schema.paths,
        ["database/schema", "extra/schema.sql"]
    );
}

#[test]
fn parses_indexing_strategy_self() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[indexing]\nstrategy = \"self\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::SelfScan));
}

#[test]
fn parses_indexing_strategy_full() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[indexing]\nstrategy = \"full\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::Full));
}

#[test]
fn parses_indexing_strategy_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[indexing]\nstrategy = \"none\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::None));
}

/// Asserted on the default value rather than through
/// [`load_config`], which merges the developer's own global
/// `.phpantom.toml` and would make the result machine-dependent.
#[test]
fn indexing_include_defaults_to_empty() {
    let dir = tempfile::tempdir().unwrap();
    let config = IndexingConfig::default();
    assert!(config.include.is_none());
    assert!(config.include_paths(dir.path()).is_empty());
}

#[test]
fn parses_indexing_include() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        "[indexing]\ninclude = [\".castor\", \".castor.stub.php\"]\n",
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(
        config.indexing.include.as_deref(),
        Some(&[".castor".to_string(), ".castor.stub.php".to_string()][..])
    );
}

#[test]
fn include_paths_resolve_relative_to_workspace_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".castor")).unwrap();

    let config = IndexingConfig {
        include: Some(vec![".castor".to_string()]),
        ..Default::default()
    };
    assert_eq!(
        config.include_paths(dir.path()),
        [dir.path().join(".castor")]
    );
}

#[test]
fn include_paths_accept_absolute_entries() {
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join(".castor.stub.php");
    std::fs::write(&stub, "<?php\n").unwrap();

    let config = IndexingConfig {
        include: Some(vec![stub.display().to_string()]),
        ..Default::default()
    };
    // Resolved against a *different* root: an absolute entry is
    // taken as-is rather than joined.
    let other_root = tempfile::tempdir().unwrap();
    assert_eq!(config.include_paths(other_root.path()), [stub]);
}

/// A stale entry must not become a phantom path handed to the
/// scanner — dropping it keeps the walk honest.
#[test]
fn include_paths_drop_missing_entries() {
    let dir = tempfile::tempdir().unwrap();
    let config = IndexingConfig {
        include: Some(vec!["does/not/exist".to_string()]),
        ..Default::default()
    };
    assert!(config.include_paths(dir.path()).is_empty());
}

#[test]
fn invalid_indexing_strategy_returns_parse_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[indexing]\nstrategy = \"bogus\"\n").unwrap();
    let result = load_config(dir.path());
    assert!(result.is_err());
}

#[test]
fn indexing_strategy_defaults_to_full() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[indexing]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.indexing.strategy(), IndexingStrategy::Full);
}

#[test]
fn indexing_strategy_display() {
    assert_eq!(IndexingStrategy::Composer.to_string(), "composer");
    assert_eq!(IndexingStrategy::SelfScan.to_string(), "self");
    assert_eq!(IndexingStrategy::Full.to_string(), "full");
    assert_eq!(IndexingStrategy::None.to_string(), "none");
}

#[test]
fn parses_formatting_php_cs_fixer_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        "[formatting]\nphp-cs-fixer = \"/usr/bin/php-cs-fixer\"\n",
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(
        config.formatting.php_cs_fixer.as_deref(),
        Some("/usr/bin/php-cs-fixer")
    );
    assert!(config.formatting.phpcbf.is_none());
}

#[test]
fn parses_formatting_phpcbf_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[formatting]\nphpcbf = \"vendor/bin/phpcbf\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(
        config.formatting.phpcbf.as_deref(),
        Some("vendor/bin/phpcbf")
    );
    assert!(config.formatting.php_cs_fixer.is_none());
}

#[test]
fn parses_formatting_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[formatting]\ntimeout = 3000\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.formatting.timeout_ms(), 3000);
}

#[test]
fn parses_blade_php_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[formatting]\nblade-php = true\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.formatting.blade_php, Some(true));
    assert!(!config.formatting.is_disabled());
}

#[test]
fn formatting_empty_string_disables_tool() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(
        &path,
        "[formatting]\nphp-cs-fixer = \"\"\nphpcbf = \"\"\npint = \"\"\n",
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.formatting.php_cs_fixer.as_deref(), Some(""));
    assert_eq!(config.formatting.phpcbf.as_deref(), Some(""));
    assert_eq!(config.formatting.pint.as_deref(), Some(""));
    assert!(config.formatting.is_disabled());
}

#[test]
fn formatting_defaults() {
    let config = Config::default();
    assert!(config.formatting.php_cs_fixer.is_none());
    assert!(config.formatting.phpcbf.is_none());
    assert!(config.formatting.timeout.is_none());
    assert_eq!(config.formatting.timeout_ms(), 10_000);
    assert!(!config.formatting.is_disabled());
}

#[test]
fn parses_phpcs_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpcs]\ncommand = \"vendor/bin/phpcs\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpcs.command.as_deref(), Some("vendor/bin/phpcs"));
}

#[test]
fn parses_phpcs_standard() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpcs]\nstandard = \"PSR1\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpcs.standard.as_deref(), Some("PSR1"));
}

#[test]
fn parses_phpcs_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpcs]\ntimeout = 20000\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpcs.timeout_ms(), 20_000);
}

#[test]
fn phpcs_empty_string_disables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CONFIG_FILE_NAME);
    std::fs::write(&path, "[phpcs]\ncommand = \"\"\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert_eq!(config.phpcs.command.as_deref(), Some(""));
    assert!(config.phpcs.is_disabled());
}

#[test]
fn phpcs_defaults() {
    let config = Config::default();
    assert!(config.phpcs.command.is_none());
    assert!(config.phpcs.standard.is_none());
    assert!(config.phpcs.timeout.is_none());
    assert_eq!(config.phpcs.timeout_ms(), 30_000);
    assert!(!config.phpcs.is_disabled());
}

#[test]
fn diagnostics_default_to_on() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(CONFIG_FILE_NAME), "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.enabled());
}

#[test]
fn switching_diagnostics_off_switches_the_workspace_pass_off() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(CONFIG_FILE_NAME),
        "[diagnostics]\nenabled = false\nworkspace = true\n",
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(!config.diagnostics.enabled());
    assert!(!config.diagnostics.workspace_enabled());
}

#[test]
fn workspace_diagnostics_default_to_off() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(CONFIG_FILE_NAME), "[diagnostics]\n").unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(!config.diagnostics.workspace_enabled());
    // The external-tool run keeps its own default; it simply has
    // nothing to chain onto until workspace diagnostics are on.
    assert!(config.diagnostics.workspace_external_enabled());
}

#[test]
fn parses_workspace_diagnostics_toggle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(CONFIG_FILE_NAME),
        "[diagnostics]\nworkspace = true\nworkspace-external = false\n",
    )
    .unwrap();
    let config = load_config(dir.path()).unwrap();
    assert!(config.diagnostics.workspace_enabled());
    assert!(!config.diagnostics.workspace_external_enabled());
}

#[test]
fn global_config_applies_when_project_has_none() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global").join(CONFIG_FILE_NAME);
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(
        &global,
        "[diagnostics]\nworkspace = true\n\n[semantic_tokens]\nmode = \"full\"\n",
    )
    .unwrap();

    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let config = load_config_from(&project, Some(&global)).unwrap();
    assert!(config.diagnostics.workspace_enabled());
    assert_eq!(config.semantic_tokens.mode(), SemanticTokensMode::Full);
}

#[test]
fn project_config_overrides_global_key_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global").join(CONFIG_FILE_NAME);
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(
        &global,
        "[diagnostics]\nworkspace = true\nextra-arguments = true\n\n[php]\nversion = \"8.1\"\n",
    )
    .unwrap();

    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(CONFIG_FILE_NAME),
        "[diagnostics]\nworkspace = false\n",
    )
    .unwrap();

    let config = load_config_from(&project, Some(&global)).unwrap();
    assert!(!config.diagnostics.workspace_enabled());
    // Untouched global keys survive the project's override.
    assert!(config.diagnostics.extra_arguments_enabled());
    assert_eq!(config.php.version.as_deref(), Some("8.1"));
}

#[test]
fn missing_global_config_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("nowhere").join(CONFIG_FILE_NAME);
    std::fs::write(
        dir.path().join(CONFIG_FILE_NAME),
        "[php]\nversion = \"8.3\"\n",
    )
    .unwrap();
    let config = load_config_from(dir.path(), Some(&global)).unwrap();
    assert_eq!(config.php.version.as_deref(), Some("8.3"));
}

#[test]
fn bad_value_in_global_config_names_the_global_file() {
    let dir = tempfile::tempdir().unwrap();
    let global = dir.path().join("global").join(CONFIG_FILE_NAME);
    std::fs::create_dir_all(global.parent().unwrap()).unwrap();
    std::fs::write(&global, "[indexing]\nstrategy = \"bogus\"\n").unwrap();

    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(CONFIG_FILE_NAME), "[php]\nversion = \"8.3\"\n").unwrap();

    let err = load_config_from(&project, Some(&global)).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains(&global.display().to_string()),
        "error should name the global config, got: {message}"
    );
}

#[test]
fn write_default_config_creates_missing_parents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a").join("b").join(CONFIG_FILE_NAME);
    assert!(write_default_config(&path).unwrap());
    assert!(std::fs::read_to_string(&path).unwrap().contains("#:schema"));
    assert!(
        !write_default_config(&path).unwrap(),
        "an existing file must not be rewritten"
    );
}

#[test]
fn merge_toml_overlay_wins() {
    let mut base: toml::Table = toml::from_str("[php]\nversion = \"8.2\"\n").unwrap();
    let overlay: toml::Table = toml::from_str("[php]\nversion = \"8.4\"\n").unwrap();
    merge_toml(&mut base, overlay);
    let config: Config = base.try_into().unwrap();
    assert_eq!(config.php.version.as_deref(), Some("8.4"));
}

#[test]
fn merge_toml_base_preserved_when_overlay_missing() {
    let mut base: toml::Table =
        toml::from_str("[php]\nversion = \"8.2\"\n\n[phpstan]\ntimeout = 30000\n").unwrap();
    let overlay: toml::Table = toml::from_str("[phpstan]\ncommand = \"phpstan\"\n").unwrap();
    merge_toml(&mut base, overlay);
    let config: Config = base.try_into().unwrap();
    assert_eq!(config.php.version.as_deref(), Some("8.2"));
    assert_eq!(config.phpstan.command.as_deref(), Some("phpstan"));
    assert_eq!(config.phpstan.timeout_ms(), 30_000);
}

#[test]
fn merge_toml_deep_merge_within_section() {
    let mut base: toml::Table =
        toml::from_str("[formatting]\ntimeout = 5000\nphpcbf = \"/usr/bin/phpcbf\"\n").unwrap();
    let overlay: toml::Table =
        toml::from_str("[formatting]\nphp-cs-fixer = \"vendor/bin/php-cs-fixer\"\n").unwrap();
    merge_toml(&mut base, overlay);
    let config: Config = base.try_into().unwrap();
    assert_eq!(
        config.formatting.php_cs_fixer.as_deref(),
        Some("vendor/bin/php-cs-fixer")
    );
    assert_eq!(config.formatting.phpcbf.as_deref(), Some("/usr/bin/phpcbf"));
    assert_eq!(config.formatting.timeout_ms(), 5000);
}

#[test]
fn merge_toml_empty_overlay() {
    let mut base: toml::Table = toml::from_str("[php]\nversion = \"8.3\"\n").unwrap();
    let overlay: toml::Table = toml::Table::new();
    merge_toml(&mut base, overlay);
    let config: Config = base.try_into().unwrap();
    assert_eq!(config.php.version.as_deref(), Some("8.3"));
}

#[test]
fn merge_toml_empty_base() {
    let mut base = toml::Table::new();
    let overlay: toml::Table = toml::from_str("[diagnostics]\nextra-arguments = true\n").unwrap();
    merge_toml(&mut base, overlay);
    let config: Config = base.try_into().unwrap();
    assert!(config.diagnostics.extra_arguments_enabled());
}

#[test]
fn merge_toml_overlay_replaces_non_table_with_value() {
    let mut base: toml::Table = toml::from_str("[indexing]\nstrategy = \"composer\"\n").unwrap();
    let overlay: toml::Table = toml::from_str("[indexing]\nstrategy = \"self\"\n").unwrap();
    merge_toml(&mut base, overlay);
    let config: Config = base.try_into().unwrap();
    assert_eq!(config.indexing.strategy, Some(IndexingStrategy::SelfScan));
}

#[test]
fn client_options_read_top_level_indexing() {
    let settings = serde_json::json!({
        "indexing": { "exclude": ["generated"], "extensions": ["module"] }
    });
    let options = ClientIndexingOptions::from_client_settings(&settings).unwrap();
    assert_eq!(options.exclude, ["generated"]);
    assert_eq!(options.extensions, ["module"]);
}

/// A client that pushes its whole settings tree namespaces the block
/// under the server's section name.
#[test]
fn client_options_read_a_phpantom_scoped_block() {
    let settings = serde_json::json!({
        "phpantom": { "indexing": { "exclude": ["build"] } },
        "editor": { "tabSize": 4 }
    });
    let options = ClientIndexingOptions::from_client_settings(&settings).unwrap();
    assert_eq!(options.exclude, ["build"]);
    assert!(options.extensions.is_empty());
}

/// A settings blob with no readable `indexing` block means "not
/// about file filters", not "the user cleared them". VS Code's
/// client re-pushes the whole `phpantom` section whenever any key in
/// it changes, so reading those as an empty filter set would discard
/// what the extension forwarded at startup the first time someone
/// toggled an unrelated setting.
#[test]
fn client_options_ignore_settings_without_an_indexing_block() {
    for settings in [
        serde_json::json!({}),
        serde_json::json!({ "unrelated": true }),
        serde_json::json!({ "phpantom": { "trace": { "server": "verbose" } } }),
        serde_json::json!({ "indexing": "not-an-object" }),
        serde_json::json!({ "indexing": { "exclude": "not-a-list" } }),
        serde_json::json!(null),
    ] {
        assert!(
            ClientIndexingOptions::from_client_settings(&settings).is_none(),
            "settings without a readable indexing block must be ignored: {settings}"
        );
    }
}

/// Clearing the filters again still has to be expressible, or a user
/// who removes their last exclude keeps it until they restart.
#[test]
fn an_empty_indexing_block_clears_the_filters() {
    for settings in [
        serde_json::json!({ "indexing": {} }),
        serde_json::json!({ "indexing": { "exclude": [], "extensions": [] } }),
    ] {
        let options = ClientIndexingOptions::from_client_settings(&settings)
            .expect("a present indexing block is a filter update");
        assert!(options.is_empty());
    }
}

/// The strategy key belongs to `.phpantom.toml` alone: an editor
/// forwards file filters, it does not get to switch the project's
/// indexing mode.
#[test]
fn client_options_carry_only_the_two_filter_lists() {
    let settings = serde_json::json!({
        "indexing": { "strategy": "none", "exclude": ["generated"] }
    });
    let options = ClientIndexingOptions::from_client_settings(&settings).unwrap();
    assert_eq!(options.exclude, ["generated"]);
}
