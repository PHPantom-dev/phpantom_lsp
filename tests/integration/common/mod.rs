#![allow(dead_code)]

use phpantom_lsp::Backend;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

pub fn create_test_backend() -> Backend {
    Backend::new_test()
}

/// Create a test backend with the **full embedded stub indices** loaded.
///
/// This is much slower than [`create_test_backend`] — only use it for
/// tests that specifically exercise behaviour backed by phpstorm-stubs
/// (e.g. deep inheritance through `\Exception`, built-in attributes).
pub fn create_test_backend_with_full_stubs() -> Backend {
    Backend::new_test_with_full_stubs()
}

// Minimal PHP stubs for UnitEnum and BackedEnum so that tests exercising
// the "embedded stub" code-path work without `composer install`.
static UNIT_ENUM_STUB: &str = "\
<?php
interface UnitEnum
{
    /** @return static[] */
    public static function cases(): array;
    public readonly string $name;
}
";

static BACKED_ENUM_STUB: &str = "\
<?php
interface BackedEnum extends UnitEnum
{
    public static function from(int|string $value): static;
    public static function tryFrom(int|string $value): ?static;
    public readonly int|string $value;
}
";

// ─── Function stubs ─────────────────────────────────────────────────────────
// Minimal PHP stubs for built-in functions grouped by extension/category.

static ARRAY_FUNCTIONS_STUB: &str = "\
<?php
/**
 * @param callable|null $callback
 * @param array $array
 * @param array ...$arrays
 * @return array
 */
function array_map(?callable $callback, array $array, array ...$arrays): array {}

/**
 * @param array &$array
 * @return mixed
 */
function array_pop(array &$array): mixed {}

/**
 * @param array &$array
 * @param mixed ...$values
 * @return int
 */
function array_push(array &$array, mixed ...$values): int {}

/**
 * @param string|int $key
 * @param array $array
 * @return bool
 */
function array_key_exists(string|int $key, array $array): bool {}
";

static STRING_FUNCTIONS_STUB: &str = "\
<?php
/**
 * @param string $haystack
 * @param string $needle
 * @return bool
 */
function str_contains(string $haystack, string $needle): bool {}

/**
 * @param string $string
 * @param int $offset
 * @param int|null $length
 * @return string
 */
function substr(string $string, int $offset, ?int $length = null): string {}
";

static JSON_FUNCTIONS_STUB: &str = "\
<?php
/**
 * @param string $json
 * @param bool|null $associative
 * @param int $depth
 * @param int $flags
 * @return mixed
 */
function json_decode(string $json, ?bool $associative = null, int $depth = 512, int $flags = 0): mixed {}
";

static DATE_FUNCTIONS_STUB: &str = "\
<?php
/**
 * @param string|null $datetime
 * @param DateTimeZone|null $timezone
 * @return DateTime|false
 */
function date_create(?string $datetime = \"now\", ?DateTimeZone $timezone = null): DateTime|false {}
";

static SIMPLEXML_FUNCTIONS_STUB: &str = "\
<?php
/**
 * @param string $data
 * @param string|null $class_name
 * @param int $options
 * @param string $namespace_or_prefix
 * @param bool $is_prefix
 * @return SimpleXMLElement|false
 */
function simplexml_load_string(string $data, ?string $class_name = null, int $options = 0, string $namespace_or_prefix = \"\", bool $is_prefix = false): SimpleXMLElement|false {}
";

static PCRE_FUNCTIONS_STUB: &str = "\
<?php
/**
 * @param string $pattern
 * @param string $subject
 * @param array|null &$matches
 * @param int $flags
 * @param int $offset
 * @return int|false
 */
function preg_match(string $pattern, string $subject, ?array &$matches = null, int $flags = 0, int $offset = 0): int|false {}
";

// ─── Class stubs ────────────────────────────────────────────────────────────

static DATETIME_CLASS_STUB: &str = "\
<?php
class DateTime
{
    public function __construct(?string $datetime = \"now\", ?DateTimeZone $timezone = null) {}

    /**
     * @param string $format
     * @return string
     */
    public function format(string $format): string {}

    /**
     * @param string $modifier
     * @return DateTime|false
     */
    public function modify(string $modifier): DateTime|false {}

    /**
     * @return int
     */
    public function getTimestamp(): int {}

    /**
     * @param int $year
     * @param int $month
     * @param int $day
     * @return DateTime
     */
    public function setDate(int $year, int $month, int $day): DateTime {}

    /**
     * @param int $hour
     * @param int $minute
     * @param int $second
     * @param int $microsecond
     * @return DateTime
     */
    public function setTime(int $hour, int $minute, int $second = 0, int $microsecond = 0): DateTime {}
}
";

static SIMPLEXMLELEMENT_CLASS_STUB: &str = "\
<?php
class SimpleXMLElement
{
    /**
     * @param string $expression
     * @return array|false|null
     */
    public function xpath(string $expression): array|false|null {}

    /**
     * @param string|null $namespaceOrPrefix
     * @param bool $isPrefix
     * @return SimpleXMLElement|null
     */
    public function children(?string $namespaceOrPrefix = null, bool $isPrefix = false): ?SimpleXMLElement {}

    /**
     * @param string|null $namespaceOrPrefix
     * @param bool $isPrefix
     * @return SimpleXMLElement|null
     */
    public function attributes(?string $namespaceOrPrefix = null, bool $isPrefix = false): ?SimpleXMLElement {}

    /**
     * @param string $qualifiedName
     * @param string|null $value
     * @param string|null $namespace
     * @return SimpleXMLElement|null
     */
    public function addChild(string $qualifiedName, ?string $value = null, ?string $namespace = null): ?SimpleXMLElement {}

    /**
     * @return string
     */
    public function getName(): string {}
}
";

// ─── stdClass stub ──────────────────────────────────────────────────────────

static STDCLASS_STUB: &str = "\
<?php
/**
 * Created by typecasting to object.
 * @link https://php.net/manual/en/reserved.classes.php
 */
class stdClass {}
";

// ─── Closure class stub ─────────────────────────────────────────────────────

static CLOSURE_CLASS_STUB: &str = "\
<?php
/**
 * Class used to represent anonymous functions.
 * @link https://php.net/manual/en/class.closure.php
 */
final class Closure
{
    private function __construct() {}

    /**
     * @param callable $callback
     * @return Closure
     */
    public static function fromCallable(callable $callback): Closure {}

    /**
     * @param object|null $newThis
     * @param string|null $newScope
     * @return Closure|null
     */
    public function bindTo(?object $newThis, ?string $newScope = \"static\"): ?Closure {}

    /**
     * @param Closure|null $closure
     * @param object|null $newThis
     * @param string|null $newScope
     * @return Closure|null
     */
    public static function bind(?Closure $closure, ?object $newThis, ?string $newScope = \"static\"): ?Closure {}

    /**
     * @param mixed ...$args
     * @return mixed
     */
    public function call(object $newThis, mixed ...$args): mixed {}

    public function __invoke(): mixed {}
}
";

// ─── Exception class stubs ──────────────────────────────────────────────────

static EXCEPTION_CLASS_STUB: &str = "\
<?php
class Exception implements Throwable
{
    public function __construct(string $message = \"\", int $code = 0, ?Throwable $previous = null) {}

    /**
     * @return string
     */
    final public function getMessage(): string {}

    /**
     * @return int
     */
    final public function getCode(): int {}

    /**
     * @return string
     */
    final public function getFile(): string {}

    /**
     * @return int
     */
    final public function getLine(): int {}

    /**
     * @return array
     */
    final public function getTrace(): array {}

    /**
     * @return string
     */
    final public function getTraceAsString(): string {}

    /**
     * @return ?Throwable
     */
    final public function getPrevious(): ?Throwable {}

    /**
     * @return string
     */
    public function __toString(): string {}
}
";

static RUNTIME_EXCEPTION_CLASS_STUB: &str = "\
<?php
class RuntimeException extends Exception {}
";

// ─── Constant stubs ─────────────────────────────────────────────────────────

static CONSTANTS_STUB: &str = "\
<?php
define('PHP_EOL', \"\\n\");
define('PHP_INT_MAX', 9223372036854775807);
define('PHP_INT_MIN', -9223372036854775808);
define('PHP_MAJOR_VERSION', 8);
define('SORT_ASC', 4);
define('SORT_DESC', 3);
";

/// Create a test backend whose `stub_index` contains minimal `Exception`
/// and `RuntimeException` stubs.  This makes catch-variable tests fully
/// self-contained — they work without phpstorm-stubs installed.
pub fn create_test_backend_with_exception_stubs() -> Backend {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("Exception", EXCEPTION_CLASS_STUB);
    stubs.insert("RuntimeException", RUNTIME_EXCEPTION_CLASS_STUB);
    Backend::new_test_with_stubs(stubs)
}

/// Create a test backend whose `stub_index` contains a minimal `stdClass`
/// stub.  This makes hover tests that resolve `\stdClass` from stubs
/// self-contained — they work without phpstorm-stubs installed.
pub fn create_test_backend_with_stdclass_stub() -> Backend {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("stdClass", STDCLASS_STUB);
    Backend::new_test_with_stubs(stubs)
}

/// Create a test backend whose `stub_index` contains a minimal `Closure`
/// stub.  This makes hover tests that resolve `\Closure` from stubs
/// self-contained — they work without phpstorm-stubs installed.
pub fn create_test_backend_with_closure_stub() -> Backend {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("Closure", CLOSURE_CLASS_STUB);
    Backend::new_test_with_stubs(stubs)
}

/// Create a test backend whose `stub_index` contains minimal `UnitEnum`
/// and `BackedEnum` stubs.  This makes "embedded stub" tests fully
/// self-contained — they no longer require a prior `composer install`.
pub fn create_test_backend_with_stubs() -> Backend {
    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("UnitEnum", UNIT_ENUM_STUB);
    stubs.insert("BackedEnum", BACKED_ENUM_STUB);
    Backend::new_test_with_stubs(stubs)
}

/// Create a test backend with embedded PHP stubs for built-in functions,
/// classes, and constants.  This makes the stub-function tests fully
/// self-contained — they work whether or not phpstorm-stubs are installed.
pub fn create_test_backend_with_function_stubs() -> Backend {
    // ── Class stubs ──
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("DateTime", DATETIME_CLASS_STUB);
    class_stubs.insert("SimpleXMLElement", SIMPLEXMLELEMENT_CLASS_STUB);
    class_stubs.insert("UnitEnum", UNIT_ENUM_STUB);
    class_stubs.insert("BackedEnum", BACKED_ENUM_STUB);

    // ── Function stubs ──
    let mut function_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    // Array functions (all point to the same source)
    function_stubs.insert("array_map", ARRAY_FUNCTIONS_STUB);
    function_stubs.insert("array_pop", ARRAY_FUNCTIONS_STUB);
    function_stubs.insert("array_push", ARRAY_FUNCTIONS_STUB);
    function_stubs.insert("array_key_exists", ARRAY_FUNCTIONS_STUB);
    // String functions
    function_stubs.insert("str_contains", STRING_FUNCTIONS_STUB);
    function_stubs.insert("substr", STRING_FUNCTIONS_STUB);
    // JSON functions
    function_stubs.insert("json_decode", JSON_FUNCTIONS_STUB);
    // Date functions
    function_stubs.insert("date_create", DATE_FUNCTIONS_STUB);
    // SimpleXML functions
    function_stubs.insert("simplexml_load_string", SIMPLEXML_FUNCTIONS_STUB);
    // PCRE functions
    function_stubs.insert("preg_match", PCRE_FUNCTIONS_STUB);

    // ── Constant stubs ──
    let mut constant_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    constant_stubs.insert("PHP_EOL", CONSTANTS_STUB);
    constant_stubs.insert("PHP_INT_MAX", CONSTANTS_STUB);
    constant_stubs.insert("PHP_INT_MIN", CONSTANTS_STUB);
    constant_stubs.insert("PHP_MAJOR_VERSION", CONSTANTS_STUB);
    constant_stubs.insert("SORT_ASC", CONSTANTS_STUB);
    constant_stubs.insert("SORT_DESC", CONSTANTS_STUB);

    Backend::new_test_with_all_stubs(class_stubs, function_stubs, constant_stubs)
}

/// Helper: create a temp workspace with a composer.json and PHP files,
/// then return a Backend configured with that workspace root + PSR-4 mappings.
pub fn create_psr4_workspace(
    composer_json: &str,
    files: &[(&str, &str)],
) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(dir.path().join("composer.json"), composer_json)
        .expect("failed to write composer.json");
    for (rel_path, content) in files {
        let full = dir.path().join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("failed to create dirs");
        }
        fs::write(&full, content).expect("failed to write PHP file");
    }

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());
    let backend = Backend::new_test_with_workspace(dir.path().to_path_buf(), mappings);
    (backend, dir)
}

/// Like [`create_psr4_workspace`] but the returned backend also has
/// minimal `Exception` and `RuntimeException` stubs injected.  This
/// makes cross-file catch-variable tests self-contained.
pub fn create_psr4_workspace_with_exception_stubs(
    composer_json: &str,
    files: &[(&str, &str)],
) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(dir.path().join("composer.json"), composer_json)
        .expect("failed to write composer.json");
    for (rel_path, content) in files {
        let full = dir.path().join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("failed to create dirs");
        }
        fs::write(&full, content).expect("failed to write PHP file");
    }

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());

    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("Exception", EXCEPTION_CLASS_STUB);
    stubs.insert("RuntimeException", RUNTIME_EXCEPTION_CLASS_STUB);

    let backend = Backend::new_test_with_stubs(stubs);
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    *backend.psr4_mappings().write() = mappings;
    (backend, dir)
}

/// Like [`create_psr4_workspace`] but the returned backend also has
/// minimal `UnitEnum` and `BackedEnum` stubs injected.  This makes
/// cross-file enum tests self-contained.
pub fn create_psr4_workspace_with_enum_stubs(
    composer_json: &str,
    files: &[(&str, &str)],
) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(dir.path().join("composer.json"), composer_json)
        .expect("failed to write composer.json");
    for (rel_path, content) in files {
        let full = dir.path().join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("failed to create dirs");
        }
        fs::write(&full, content).expect("failed to write PHP file");
    }

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());

    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    stubs.insert("UnitEnum", UNIT_ENUM_STUB);
    stubs.insert("BackedEnum", BACKED_ENUM_STUB);

    let backend = Backend::new_test_with_stubs(stubs);
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    *backend.psr4_mappings().write() = mappings;
    (backend, dir)
}

/// Like [`create_psr4_workspace`] but the returned backend's `stub_index`
/// is seeded with the given `(name, source)` stub entries.  Useful for
/// tests that need a global stub class (e.g. the SPL `Iterator`) to
/// coexist with a same-named project class.
pub fn create_psr4_workspace_with_stubs(
    composer_json: &str,
    files: &[(&str, &str)],
    stub_entries: &[(&'static str, &'static str)],
) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::write(dir.path().join("composer.json"), composer_json)
        .expect("failed to write composer.json");
    for (rel_path, content) in files {
        let full = dir.path().join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("failed to create dirs");
        }
        fs::write(&full, content).expect("failed to write PHP file");
    }

    let (mappings, _vendor_dir) = phpantom_lsp::composer::parse_composer_json(dir.path());

    let mut stubs: HashMap<&'static str, &'static str> = HashMap::new();
    for (name, source) in stub_entries {
        stubs.insert(name, source);
    }

    let backend = Backend::new_test_with_stubs(stubs);
    *backend.workspace_root().write() = Some(dir.path().to_path_buf());
    *backend.psr4_mappings().write() = mappings;
    (backend, dir)
}

// ── Shared code-action test helpers ─────────────────────────────────────────

/// Inject a PHPStan diagnostic into the backend's cache and return it.
pub fn inject_phpstan_diag(
    backend: &Backend,
    uri: &str,
    line: u32,
    message: &str,
    identifier: &str,
) -> Diagnostic {
    let diag = Diagnostic {
        range: Range {
            start: Position::new(line, 0),
            end: Position::new(line, 80),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(identifier.to_string())),
        source: Some("PHPStan".to_string()),
        message: message.to_string(),
        ..Default::default()
    };
    {
        let mut cache = backend.phpstan_last_diags().lock();
        cache.entry(uri.to_string()).or_default().push(diag.clone());
    }
    diag
}

/// Send a code action request at a specific line and character (point range).
pub fn get_code_actions_at(
    backend: &Backend,
    uri: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<CodeActionOrCommand> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(line, character),
            end: Position::new(line, character),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };
    backend.handle_code_action(uri, content, &params)
}

/// Send a code action request spanning an entire line (columns 0–80).
pub fn get_code_actions_on_line(
    backend: &Backend,
    uri: &str,
    content: &str,
    line: u32,
) -> Vec<CodeActionOrCommand> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range: Range {
            start: Position::new(line, 0),
            end: Position::new(line, 80),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
    };
    backend.handle_code_action(uri, content, &params)
}

/// Find a code action by title prefix.
pub fn find_action<'a>(actions: &'a [CodeActionOrCommand], prefix: &str) -> Option<&'a CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with(prefix) => Some(ca),
        _ => None,
    })
}

/// Resolve a deferred code action by storing file content in open_files
/// and calling resolve_code_action.
pub fn resolve_action(
    backend: &Backend,
    uri: &str,
    content: &str,
    action: &CodeAction,
) -> CodeAction {
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), Arc::new(content.to_string()));
    let (resolved, _) = backend.resolve_code_action(action.clone());
    assert!(
        resolved.edit.is_some(),
        "resolved action should have an edit, title: {}",
        resolved.title
    );
    resolved
}

/// Extract all text edits from a resolved code action.
pub fn extract_edits(action: &CodeAction) -> Vec<TextEdit> {
    let edit = action.edit.as_ref().expect("action should have an edit");
    let changes = edit.changes.as_ref().expect("edit should have changes");
    changes.values().flat_map(|v| v.iter()).cloned().collect()
}

/// Apply text edits to content, producing the resulting source.
pub fn apply_edits(content: &str, edits: &[TextEdit]) -> String {
    let mut result = content.to_string();
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        b.range
            .start
            .line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });
    for edit in sorted {
        let start = lsp_pos_to_offset(&result, edit.range.start);
        let end = lsp_pos_to_offset(&result, edit.range.end);
        result.replace_range(start..end, &edit.new_text);
    }
    result
}

/// Convert an LSP `Position` (line, character) to a byte offset in `content`.
pub fn lsp_pos_to_offset(content: &str, pos: Position) -> usize {
    let mut offset = 0;
    for (i, line) in content.lines().enumerate() {
        if i == pos.line as usize {
            return offset + pos.character as usize;
        }
        offset += line.len() + 1;
    }
    content.len()
}
