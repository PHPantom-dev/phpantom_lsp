#![allow(dead_code)]

pub mod lsp_transport;

use phpantom_lsp::Backend;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

pub fn create_test_backend() -> Backend {
    Backend::new_test()
}

/// Open `text` in the backend as `uri` with the given LSP language id,
/// the way an editor's `textDocument/didOpen` would.
pub async fn open_document(backend: &Backend, uri: &Url, language_id: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: language_id.to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

/// [`open_document`] for a PHP file.
pub async fn open_php(backend: &Backend, uri: &Url, text: &str) {
    open_document(backend, uri, "php", text).await;
}

// ─── Completion ─────────────────────────────────────────────────────────────

/// Send a `textDocument/completion` request for a document that is already
/// open.  The one place these helpers build `CompletionParams`.
async fn completion_response(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    context: Option<CompletionContext>,
) -> Option<CompletionResponse> {
    backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context,
        })
        .await
        .unwrap()
}

/// Flatten a completion response into its items, keeping "the server had
/// nothing to offer" (`None`) distinct from "the server offered an empty
/// list".
fn response_items(response: Option<CompletionResponse>) -> Option<Vec<CompletionItem>> {
    match response {
        Some(CompletionResponse::Array(items)) => Some(items),
        Some(CompletionResponse::List(list)) => Some(list.items),
        None => None,
    }
}

pub fn item_labels(items: Vec<CompletionItem>) -> Vec<String> {
    items.into_iter().map(|item| item.label).collect()
}

/// The labels a completion response offered, none when the server had
/// nothing to say.
pub fn response_labels(response: Option<CompletionResponse>) -> Vec<String> {
    item_labels(response_items(response).unwrap_or_default())
}

/// Open `text` as PHP and return the raw completion response at the position.
pub async fn complete_response_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Option<CompletionResponse> {
    open_php(backend, uri, text).await;
    completion_response(backend, uri, line, character, None).await
}

/// [`complete_at`] that distinguishes no response from an empty one.
pub async fn complete_at_raw(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Option<Vec<CompletionItem>> {
    response_items(complete_response_at(backend, uri, text, line, character).await)
}

/// Open `text` as PHP and return the completion items offered at the position.
pub async fn complete_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    complete_at_raw(backend, uri, text, line, character)
        .await
        .unwrap_or_default()
}

/// [`complete_at`] reduced to the item labels.
pub async fn complete_labels_at(
    backend: &Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<String> {
    item_labels(complete_at(backend, uri, text, line, character).await)
}

/// [`complete_at`] for a document the test opened itself, which is what
/// tests that open with a non-PHP language id or exercise edits before
/// completing need.
pub async fn complete_at_opened(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    response_items(completion_response(backend, uri, line, character, None).await)
        .unwrap_or_default()
}

/// [`complete_at_opened`] reduced to the item labels.
pub async fn complete_labels_at_opened(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<String> {
    item_labels(complete_at_opened(backend, uri, line, character).await)
}

/// [`complete_at_opened`] for a request an editor sends because the user just
/// typed `trigger_character`, rather than an explicitly invoked completion.
/// [`complete_at_opened_with_trigger`], reduced to the labels offered.
pub async fn complete_labels_at_opened_with_trigger(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    trigger_character: &str,
) -> Vec<String> {
    item_labels(
        complete_at_opened_with_trigger(backend, uri, line, character, trigger_character).await,
    )
}

pub async fn complete_at_opened_with_trigger(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    trigger_character: &str,
) -> Vec<CompletionItem> {
    let context = CompletionContext {
        trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
        trigger_character: Some(trigger_character.to_string()),
    };
    response_items(completion_response(backend, uri, line, character, Some(context)).await)
        .unwrap_or_default()
}

/// Run an async LSP request on a thread sized like the server's own
/// AST-walking threads.
///
/// The server gives every thread that parses or walks a PHP AST
/// [`phpantom_lsp::PARSE_WORKER_STACK_SIZE`], while libtest threads get
/// the 2 MiB default and an unoptimised test build uses far larger
/// frames than the release binary.  Tests that feed the walker deeply
/// nested PHP need the production budget to be measuring the same thing
/// the server does.
pub fn with_parse_worker_stack<T, F>(body: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    std::thread::Builder::new()
        .stack_size(phpantom_lsp::PARSE_WORKER_STACK_SIZE)
        .spawn(body)
        .expect("spawn parse-worker-sized test thread")
        .join()
        .expect("test body panicked")
}

/// Block on an async LSP body from a synchronous test, on a runtime
/// configured like the server's (see [`with_parse_worker_stack`] for why
/// the stack size matters).
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_stack_size(phpantom_lsp::PARSE_WORKER_STACK_SIZE)
        .build()
        .expect("build test runtime")
        .block_on(future)
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

/// The path of `relative` inside the backend's workspace root, for the
/// workspaces [`create_psr4_workspace`] and friends lay out on disk.
pub fn workspace_path(backend: &Backend, relative: &str) -> PathBuf {
    let root = backend.workspace_root().read().clone().unwrap();
    root.join(relative)
}

/// [`workspace_path`] as the URI an editor would open the file under.
pub fn workspace_uri(backend: &Backend, relative: &str) -> Url {
    Url::from_file_path(workspace_path(backend, relative)).unwrap()
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
    inject_phpstan_diag_with_data(backend, uri, line, message, identifier, None)
}

/// [`inject_phpstan_diag`] carrying the `data` payload the real proxy
/// attaches, e.g. `{"ignorable": false}` for a rule that cannot be
/// silenced with an ignore comment.
pub fn inject_phpstan_diag_with_data(
    backend: &Backend,
    uri: &str,
    line: u32,
    message: &str,
    identifier: &str,
    data: Option<serde_json::Value>,
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
        data,
        ..Default::default()
    };
    {
        let mut cache = backend.phpstan_last_diags().lock();
        cache.entry(uri.to_string()).or_default().push(diag.clone());
    }
    diag
}

/// Send a code action request for an arbitrary range.
pub fn get_code_actions_in_range(
    backend: &Backend,
    uri: &str,
    content: &str,
    range: Range,
) -> Vec<CodeActionOrCommand> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: uri.parse().unwrap(),
        },
        range,
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

/// Send a code action request at a specific line and character (point range).
pub fn get_code_actions_at(
    backend: &Backend,
    uri: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Vec<CodeActionOrCommand> {
    let pos = Position::new(line, character);
    get_code_actions_in_range(backend, uri, content, Range::new(pos, pos))
}

/// Send a code action request spanning an entire line (columns 0–80).
pub fn get_code_actions_on_line(
    backend: &Backend,
    uri: &str,
    content: &str,
    line: u32,
) -> Vec<CodeActionOrCommand> {
    get_code_actions_in_range(
        backend,
        uri,
        content,
        Range::new(Position::new(line, 0), Position::new(line, 80)),
    )
}

/// Find a code action by title prefix.
pub fn find_action<'a>(actions: &'a [CodeActionOrCommand], prefix: &str) -> Option<&'a CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with(prefix) => Some(ca),
        _ => None,
    })
}

/// Find a code action by exact title.
pub fn find_action_titled<'a>(
    actions: &'a [CodeActionOrCommand],
    title: &str,
) -> Option<&'a CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title == title => Some(ca),
        _ => None,
    })
}

/// Find a code action whose title contains `needle`.
pub fn find_action_containing<'a>(
    actions: &'a [CodeActionOrCommand],
    needle: &str,
) -> Option<&'a CodeAction> {
    actions.iter().find_map(|a| match a {
        CodeActionOrCommand::CodeAction(ca) if ca.title.contains(needle) => Some(ca),
        _ => None,
    })
}

/// Find all code actions whose title starts with `prefix`.
pub fn find_actions<'a>(actions: &'a [CodeActionOrCommand], prefix: &str) -> Vec<&'a CodeAction> {
    actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.starts_with(prefix) => Some(ca),
            _ => None,
        })
        .collect()
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

/// Extract the single text edit's replacement text from a resolved code action.
pub fn extract_edit_text(action: &CodeAction) -> String {
    let mut edits = extract_edits(action);
    assert_eq!(edits.len(), 1, "expected exactly one text edit");
    edits.pop().unwrap().new_text
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

/// Apply a workspace edit that touches a single URI to `content`.
pub fn apply_workspace_edit(content: &str, edit: &WorkspaceEdit) -> String {
    let changes = edit.changes.as_ref().expect("edit should have changes");
    let edits = changes
        .values()
        .next()
        .expect("should have edits for one URI");
    apply_edits(content, edits)
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

// ── Shared rename test helpers ──────────────────────────────────────────────

/// Tell the backend the client accepts `RenameFile` and `CreateFile`
/// resource operations in a workspace edit, the way an editor's
/// `initialize` handshake does.
///
/// A class rename only emits the `RenameFile` operation that moves the
/// declaring file when the client advertises it here.
pub async fn initialize_with_resource_operations(backend: &Backend) {
    backend
        .initialize(InitializeParams {
            capabilities: ClientCapabilities {
                workspace: Some(WorkspaceClientCapabilities {
                    workspace_edit: Some(WorkspaceEditClientCapabilities {
                        resource_operations: Some(vec![
                            ResourceOperationKind::Rename,
                            ResourceOperationKind::Create,
                        ]),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("initialize should succeed");
}

/// Helper: send a prepare-rename request and return the response.
pub async fn prepare_rename(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<PrepareRenameResponse> {
    let params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position { line, character },
    };

    backend.prepare_rename(params).await.unwrap()
}

/// Helper: send a rename request and return the workspace edit.
pub async fn rename(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    new_name: &str,
) -> Option<WorkspaceEdit> {
    rename_result(backend, uri, line, character, new_name)
        .await
        .expect("rename was refused")
}

/// Like [`rename`] but keeps the refusal, so a test can assert on the
/// message the user is shown.
pub async fn rename_result(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
    new_name: &str,
) -> std::result::Result<Option<WorkspaceEdit>, String> {
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        new_name: new_name.to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    backend
        .rename(params)
        .await
        .map_err(|e| e.message.to_string())
}

/// Line and character of the first occurrence of `needle` in `haystack`.
pub fn line_char_of(haystack: &str, needle: &str) -> (u32, u32) {
    for (line_idx, line) in haystack.lines().enumerate() {
        if let Some(char_idx) = line.find(needle) {
            return (line_idx as u32, char_idx as u32);
        }
    }
    panic!("needle not found: {needle}");
}

/// Collect all text edits for a given URI from a WorkspaceEdit.
pub fn edits_for_uri(edit: &WorkspaceEdit, uri: &Url) -> Vec<TextEdit> {
    if let Some(changes) = edit.changes.as_ref() {
        return changes.get(uri).cloned().unwrap_or_default();
    }
    let Some(DocumentChanges::Operations(ops)) = &edit.document_changes else {
        return Vec::new();
    };
    ops.iter()
        .filter_map(|op| match op {
            DocumentChangeOperation::Edit(e) if e.text_document.uri == *uri => Some(&e.edits),
            _ => None,
        })
        .flatten()
        .map(|e| match e {
            OneOf::Left(e) => e.clone(),
            OneOf::Right(e) => e.text_edit.clone(),
        })
        .collect()
}

/// Extract the `RenameFile` operation from a `WorkspaceEdit`, if any.
pub fn extract_rename_file(edit: &WorkspaceEdit) -> Option<&RenameFile> {
    let doc_changes = edit.document_changes.as_ref()?;
    match doc_changes {
        DocumentChanges::Operations(ops) => {
            for op in ops {
                if let DocumentChangeOperation::Op(ResourceOp::Rename(rf)) = op {
                    return Some(rf);
                }
            }
            None
        }
        _ => None,
    }
}

/// Collect all text edits for a given URI from a `WorkspaceEdit` that uses
/// `document_changes` (the `DocumentChanges::Operations` variant).
pub fn doc_change_edits_for_uri(edit: &WorkspaceEdit, uri: &Url) -> Vec<TextEdit> {
    let Some(DocumentChanges::Operations(ops)) = &edit.document_changes else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for op in ops {
        if let DocumentChangeOperation::Edit(tde) = op
            && tde.text_document.uri == *uri
        {
            for e in &tde.edits {
                match e {
                    OneOf::Left(te) => result.push(te.clone()),
                    OneOf::Right(ate) => result.push(TextEdit {
                        range: ate.text_edit.range,
                        new_text: ate.text_edit.new_text.clone(),
                    }),
                }
            }
        }
    }
    result
}

// ─── Shared assertions helpers ──────────────────────────────────────────────

/// [`open_php`] for a URI written as a string.
pub async fn open_php_str(backend: &Backend, uri: &str, text: &str) {
    open_php(backend, &Url::parse(uri).unwrap(), text).await;
}

/// The labels of `items`, in the order they were offered.
pub fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

/// The method items among `items`, by the name completion inserts.
pub fn method_names(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect()
}

/// The property items among `items`, by the name completion inserts.
pub fn property_names(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect()
}

/// The position just past the first occurrence of `needle` in `content`,
/// in LSP coordinates (UTF-16 code units from the start of the line).
pub fn position_after(content: &str, needle: &str) -> Position {
    let offset = content
        .find(needle)
        .unwrap_or_else(|| panic!("needle not found: {needle}"))
        + needle.len();
    offset_position(content, offset)
}

/// The position of the first occurrence of `needle` in `content`, in LSP
/// coordinates.
pub fn position_of(content: &str, needle: &str) -> Position {
    let offset = content
        .find(needle)
        .unwrap_or_else(|| panic!("needle not found: {needle}"));
    offset_position(content, offset)
}

fn offset_position(content: &str, offset: usize) -> Position {
    let before = &content[..offset];
    let line = before.matches('\n').count() as u32;
    let character = before
        .rsplit('\n')
        .next()
        .unwrap_or(before)
        .encode_utf16()
        .count() as u32;
    Position { line, character }
}

// ─── Hover ──────────────────────────────────────────────────────────────────

/// Parse `content` as `uri` and answer the hover at the position, going
/// through the handler directly (no LSP round trip).
pub fn hover_at(
    backend: &Backend,
    uri: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Option<Hover> {
    backend.update_ast(uri, content);
    backend.handle_hover(uri, content, Position { line, character })
}

/// The markdown of a hover; panics on any other content kind.
pub fn hover_text(hover: &Hover) -> &str {
    match &hover.contents {
        HoverContents::Markup(markup) => &markup.value,
        other => panic!("expected markup hover, got {other:?}"),
    }
}

/// Send a `textDocument/hover` request for a document that is already
/// open and return its text, whichever content kind it came as.  `None`
/// when nothing hovers.
pub async fn hover_text_at(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<String> {
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()?;
    Some(match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        HoverContents::Scalar(MarkedString::String(s)) => s,
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value,
        HoverContents::Array(items) => items
            .into_iter()
            .map(|item| match item {
                MarkedString::String(s) => s,
                MarkedString::LanguageString(ls) => ls.value,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

/// [`hover_text_at`] for a position that must hover.
pub async fn markup_hover_at(backend: &Backend, uri: &Url, line: u32, character: u32) -> String {
    hover_text_at(backend, uri, line, character)
        .await
        .unwrap_or_else(|| panic!("expected a hover at {line}:{character}"))
}

/// The items among `items` whose kind is `kind`.
pub fn items_of_kind(items: &[CompletionItem], kind: CompletionItemKind) -> Vec<&CompletionItem> {
    items.iter().filter(|i| i.kind == Some(kind)).collect()
}

/// The class items among `items`.
pub fn class_items(items: &[CompletionItem]) -> Vec<&CompletionItem> {
    items_of_kind(items, CompletionItemKind::CLASS)
}

/// The `filter_text` of every item that carries one.
pub fn filter_texts(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect()
}

/// Send a `textDocument/definition` request for a document that is
/// already open.
pub async fn goto_definition_at(
    backend: &Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<GotoDefinitionResponse> {
    backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap()
}

/// The "Undefined variable" messages reported on an open Blade template,
/// read against the virtual PHP it lowers to.
pub fn blade_undefined_variables(backend: &Backend, uri: &Url) -> Vec<String> {
    let virtual_php = backend
        .blade_virtual_php(uri.as_str())
        .expect("blade virtual content");
    let mut diags = Vec::new();
    backend.collect_undefined_variable_diagnostics(uri.as_str(), &virtual_php, &mut diags);
    diags
        .into_iter()
        .filter(|d| d.message.contains("Undefined variable"))
        .map(|d| d.message)
        .collect()
}

/// Parse `text` as `uri` and return the `unknown_member` diagnostics the
/// slow pass reports, with its per-file scope cache live.
pub fn unknown_member_diagnostics_with_scope_cache(
    backend: &Backend,
    uri: &str,
    text: &str,
) -> Vec<Diagnostic> {
    backend.update_ast(uri, text);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, text, &mut out);
    out.retain(|d| {
        d.code
            .as_ref()
            .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
    });
    out
}

// ─── Go-to-definition ───────────────────────────────────────────────────────

/// The targets of a go-to-definition response as plain locations, whatever
/// shape the server answered in.
pub fn definition_locations(response: Option<GotoDefinitionResponse>) -> Vec<Location> {
    match response {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
        Some(GotoDefinitionResponse::Array(locations)) => locations,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
    }
}

/// The URI of the first target of a go-to-definition response.
pub fn definition_uri(response: &GotoDefinitionResponse) -> &Url {
    match response {
        GotoDefinitionResponse::Scalar(location) => &location.uri,
        GotoDefinitionResponse::Array(locations) => &locations[0].uri,
        GotoDefinitionResponse::Link(links) => &links[0].target_uri,
    }
}

// ─── Blade ──────────────────────────────────────────────────────────────────

/// The `composer.json` of a workspace holding an application under `app/`
/// plus stubs of the two component base classes, laid out where
/// [`ILLUMINATE_COMPONENT_STUB`] and [`LIVEWIRE_COMPONENT_STUB`] expect
/// to be written (`stubs/Illuminate/View/Component.php` and
/// `stubs/Livewire/Component.php`).
pub const BLADE_COMPONENT_COMPOSER: &str = r#"{"autoload": {"psr-4": {
    "App\\": "app/",
    "Illuminate\\": "stubs/Illuminate/",
    "Livewire\\": "stubs/Livewire/"
}}}"#;

/// A stand-in for `Illuminate\View\Component`, the base class of a
/// class-based Blade component. Besides `render()`, it carries the
/// framework members a component inherits but never exposes to its
/// template.
pub const ILLUMINATE_COMPONENT_STUB: &str = "<?php\nnamespace Illuminate\\View;\n\
    abstract class Component {\n\
        public $componentName;\n\
        public $attributes;\n\
        public function render() {}\n\
        public function data() {}\n\
        public function shouldRender() {}\n\
    }\n";

/// Strip the `§` cursor marker from `content`, returning the text an
/// editor would hold and the position the marker stood at.
pub fn split_cursor(content: &str) -> (String, Position) {
    let offset = content.find('§').expect("test source needs a § cursor");
    let before = &content[..offset];
    let line = before.matches('\n').count() as u32;
    let character = before.rsplit('\n').next().unwrap_or("").chars().count() as u32;
    (content.replace('§', ""), Position { line, character })
}

// ── Shared Laravel fixtures ─────────────────────────────────────────────────

/// A Laravel `composer.json` whose `App\` namespace maps to `src/`.
pub const LARAVEL_SRC_COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

/// A Laravel `composer.json` whose `App\` namespace maps to `app/`, the
/// layout a real Laravel application uses.
pub const LARAVEL_APP_COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

/// The `App\` to `app/` mapping alone, for a project that needs the
/// autoload layout but not the framework dependency.
pub const APP_PSR4_COMPOSER: &str = r#"{"autoload": {"psr-4": {"App\\": "app/"}}}"#;

/// A minimal Eloquent-shaped model for templates to render.
pub const USER_MODEL_STUB: &str =
    "<?php\nnamespace App\\Models;\nclass User { public string $email = ''; }\n";

/// Laravel's `app()` helper, with the conditional return type that makes
/// `app(Foo::class)` resolve to `Foo`.
pub const APP_HELPERS_PHP: &str = r#"<?php
/**
 * @template TClass
 * @param string|class-string<TClass> $abstract
 * @return ($abstract is class-string<TClass> ? TClass : \Illuminate\Foundation\Application)
 */
function app($abstract = null, array $parameters = [])
{
}
"#;

/// A stand-in for `Illuminate\Foundation\Http\FormRequest`.
pub const FORM_REQUEST_STUB: &str = "\
<?php
namespace Illuminate\\Foundation\\Http;
use Illuminate\\Http\\Request;
class FormRequest extends Request {
    public function rules(): array { return []; }
}
";

/// A consumer class whose one method runs `body` and then reads the
/// variable it bound, the shape container-resolution tests assert on.
pub fn consumer_class(body: &str) -> String {
    format!(
        "<?php\nnamespace App;\nclass Consumer {{\n    public function go(): void {{\n        $x = {body};\n        $x;\n    }}\n}}\n"
    )
}

/// A stand-in for `Livewire\Component`, the base class of a Livewire
/// component.
pub const LIVEWIRE_COMPONENT_STUB: &str = "<?php\nnamespace Livewire;\n\
    abstract class Component {\n\
        public function render() {}\n\
        public function dispatch(string $event) {}\n\
    }\n";

/// Open the Blade template at `relative` in the backend's workspace from
/// disk, the way an editor opening it with language id `blade` would, and
/// hand back its URI.
pub async fn open_blade_template(backend: &Backend, relative: &str) -> Url {
    let path = workspace_path(backend, relative);
    let text = fs::read_to_string(&path).unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    open_document(backend, &uri, "blade", &text).await;
    uri
}

/// [`open_blade_template`] with the workspace scan already run, so provider
/// registrations and view discovery are in place before the template opens.
pub async fn open_initialized_blade_template(backend: &Backend, relative: &str) -> Url {
    backend.initialized(InitializedParams {}).await;
    open_blade_template(backend, relative).await
}

/// Open the PHP file at `relative` in the backend's workspace from disk,
/// the way an editor opening it would, and hand back its URI.
pub async fn open_php_file(backend: &Backend, relative: &str) -> Url {
    let path = workspace_path(backend, relative);
    let text = fs::read_to_string(&path).unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    open_php(backend, &uri, &text).await;
    uri
}

/// [`open_php_file`] with the workspace scan already run, so provider
/// registrations, route files, and the other Laravel discoveries are in
/// place before the file opens.
pub async fn open_initialized_php(backend: &Backend, relative: &str) -> Url {
    backend.initialized(InitializedParams {}).await;
    open_php_file(backend, relative).await
}

/// [`create_psr4_workspace`] followed by [`open_initialized_php`] on
/// `open_path`: the fixture the Laravel discovery suites start from.
pub async fn create_initialized_psr4_workspace(
    composer_json: &str,
    files: &[(&str, &str)],
    open_path: &str,
) -> (Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(composer_json, files);
    let uri = open_initialized_php(&backend, open_path).await;
    (backend, dir, uri)
}

// ─── Diagnostics ────────────────────────────────────────────────────────────

/// The diagnostics among `diags` whose `code` is the string `code`.
pub fn with_code(diags: Vec<Diagnostic>, code: &str) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == code))
        })
        .collect()
}

/// The messages of the diagnostics among `diags` whose `code` is the
/// string `code`.
pub fn messages_with_code(diags: &[Diagnostic], code: &str) -> Vec<String> {
    diags
        .iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == code))
        })
        .map(|d| d.message.clone())
        .collect()
}

/// Parse `php` as `uri` and return the messages of the slow-pass
/// diagnostics whose `code` is the string `code`.
pub fn slow_diagnostic_messages(
    backend: &Backend,
    uri: &str,
    php: &str,
    code: &str,
) -> Vec<String> {
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, php, &mut out);
    messages_with_code(&out, code)
}

/// Parse `php` as `file:///test.php` on `backend` and run one diagnostic
/// collector over it, e.g. `Backend::collect_unused_variable_diagnostics`.
pub fn collect_diagnostics_with(
    backend: &Backend,
    php: &str,
    collect: impl Fn(&Backend, &str, &str, &mut Vec<Diagnostic>),
) -> Vec<Diagnostic> {
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let mut out = Vec::new();
    collect(backend, uri, php, &mut out);
    out
}

// ─── Types read off hovers ──────────────────────────────────────────────────

/// The type a hover reports for the assignment at `position`: the text
/// after ` = ` on the first hover line that has one.
pub fn hover_assigned_type(
    backend: &Backend,
    uri: &str,
    content: &str,
    position: Position,
) -> String {
    let Position { line, character } = position;
    let hover = hover_at(backend, uri, content, line, character)
        .unwrap_or_else(|| panic!("no hover at {line}:{character}"));
    let text = hover_text(&hover);
    text.lines()
        .find_map(|l| l.split_once(" = ").map(|(_, ty)| ty.trim().to_string()))
        .unwrap_or_else(|| panic!("no assignment in hover at {line}:{character}: {text}"))
}

/// The type of the assignment to `var` (`$name`): the line whose trimmed
/// text starts with `$name = `, hovered on the variable.
pub fn assigned_type(backend: &Backend, uri: &str, content: &str, var: &str) -> String {
    let needle = format!("{var} = ");
    let (line, text) = content
        .lines()
        .enumerate()
        .find(|(_, l)| l.trim_start().starts_with(&needle))
        .unwrap_or_else(|| panic!("no assignment to {var} in the fixture"));
    let indent = (text.len() - text.trim_start().len()) as u32;
    let position = Position {
        line: line as u32,
        character: indent + 1,
    };
    hover_assigned_type(backend, uri, content, position)
}

/// Assert the type each named variable is assigned in `content`, as
/// [`assigned_type`] reports it, on a fresh full-stubs backend.
pub fn assert_assigned_types(content: &str, expected: &[(&str, &str)]) {
    assert_assigned_types_on(
        &create_test_backend_with_full_stubs(),
        "file:///test.php",
        content,
        expected,
    );
}

/// [`assert_assigned_types`] against a backend the caller has prepared,
/// with `content` parsed as `uri`.
pub fn assert_assigned_types_on(
    backend: &Backend,
    uri: &str,
    content: &str,
    expected: &[(&str, &str)],
) {
    for (var, want) in expected {
        assert_eq!(&assigned_type(backend, uri, content, var), want, "{var}");
    }
}

/// The type reported for the variable right after a `/*MARKER*/` comment.
pub fn type_at_marker(backend: &Backend, uri: &str, content: &str, marker: &str) -> String {
    let needle = format!("/*{marker}*/$");
    hover_assigned_type(backend, uri, content, position_after(content, &needle))
}
