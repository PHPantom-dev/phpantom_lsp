//! Data-driven fixture test runner.
//!
//! Each `.fixture` file in `tests/fixtures/` encodes a single test scenario.
//! The runner parses the file, creates a test backend, opens the PHP source,
//! fires the appropriate LSP request, and checks the declared expectations.
//!
//! See `tests/fixtures/README.md` for the fixture format specification.

use std::collections::HashMap;
use std::path::Path;

use phpantom_lsp::Backend;
use phpantom_lsp::types::PhpVersion;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

#[path = "support/fixture_project.rs"]
mod fixture_project;

// ─── Embedded stubs ─────────────────────────────────────────────────────────
// Minimal PHP stubs so that fixture tests mirror real LSP behaviour.
// Enums implicitly implement UnitEnum / BackedEnum, and the inheritance
// system needs to load these interfaces to surface `$name` and `$value`.

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

static JSON_EXCEPTION_STUB: &str = "\
<?php
class JsonException extends Exception {}
";

static VALUE_ERROR_STUB: &str = "\
<?php
class ValueError extends Error {}
";

// ─── Function stubs ─────────────────────────────────────────────────────────
// Minimal function stubs so that fixture tests can resolve return types of
// built-in functions.  These mirror the phpstorm-stubs signatures; the
// stub patch system (stub_patches.rs) upgrades some of them at load time
// (e.g. linking array_map's callback parameter to the array's element type).

static ARRAY_FUNC_STUB: &str = "\
<?php
/**
 * @template TCarry
 * @template TValue
 * @param array<TValue> $array
 * @param callable(TCarry, TValue): TCarry $callback
 * @param TCarry $initial
 * @return TCarry
 */
function array_reduce(array $array, callable $callback, mixed $initial = null): mixed {}

/**
 * @param array $array
 * @return int|float
 */
function array_sum(array $array): int|float {}

/**
 * @param array $array
 * @return int|float
 */
function array_product(array $array): int|float {}

/**
 * @template TValue
 * @param callable(TValue): mixed $callback
 * @param array<TValue> $array
 * @return array
 */
function array_map(?callable $callback, array $array): array {}

/**
 * @template TValue
 * @param array<TValue> $array
 * @param callable(TValue): bool $callback
 * @return array<TValue>
 */
function array_filter(array $array, ?callable $callback = null): array {}
";

static JSON_FUNC_STUB: &str = "\
<?php
/**
 * @throws \\ValueError
 */
function json_decode(string $json, ?bool $associative = null, int $depth = 512, int $flags = 0): mixed {}

function json_encode(mixed $value, int $flags = 0, int $depth = 512): string|false {}
";

/// The backend a fixture runs on: the full embedded stubs for the PHP
/// version a `// php:` line names, and otherwise the minimal stubs above.
fn fixture_backend(meta: &TestMeta) -> Backend {
    match meta.php_version {
        Some(version) => {
            let backend = Backend::new_test_with_full_stubs();
            backend.set_php_version(version);
            backend
        }
        None => create_fixture_backend(),
    }
}

/// Create a test backend with embedded class and function stubs pre-loaded.
fn create_fixture_backend() -> Backend {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("UnitEnum", UNIT_ENUM_STUB);
    class_stubs.insert("BackedEnum", BACKED_ENUM_STUB);
    class_stubs.insert("JsonException", JSON_EXCEPTION_STUB);
    class_stubs.insert("ValueError", VALUE_ERROR_STUB);

    let mut func_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    func_stubs.insert("array_reduce", ARRAY_FUNC_STUB);
    func_stubs.insert("array_sum", ARRAY_FUNC_STUB);
    func_stubs.insert("array_product", ARRAY_FUNC_STUB);
    func_stubs.insert("array_map", ARRAY_FUNC_STUB);
    func_stubs.insert("array_filter", ARRAY_FUNC_STUB);
    func_stubs.insert("json_decode", JSON_FUNC_STUB);
    func_stubs.insert("json_encode", JSON_FUNC_STUB);

    Backend::new_test_with_all_stubs(class_stubs, func_stubs, HashMap::new())
}

// ─── Fixture parsing ────────────────────────────────────────────────────────

/// Parsed metadata from the fixture header.
struct TestMeta {
    /// Human-readable test description (from `// test:`).
    #[allow(dead_code)]
    description: String,
    /// Which LSP feature to exercise.
    feature: Feature,
    /// Labels that must appear in completion results (from `// expect:`).
    expect: Vec<String>,
    /// Labels that must NOT appear in completion results, or substrings that
    /// must NOT appear in hover text (from `// expect_absent:`).
    expect_absent: Vec<String>,
    /// Hover assertions: `symbol => substring` (from `// expect_hover:`).
    expect_hover: Vec<(String, String)>,
    /// Definition assertions: `file:line` or `self:line` (from `// expect_definition:`).
    expect_definition: Vec<String>,
    /// Signature help label assertion (from `// expect_sig_label:`).
    expect_sig_label: Option<String>,
    /// Signature help active parameter index (from `// expect_sig_active:`).
    expect_sig_active: Option<u32>,
    /// Signature help parameter label assertions (from `// expect_sig_param:`).
    expect_sig_params: Vec<String>,
    /// If set, the test should be ignored with this reason.
    ignore: Option<String>,
    /// The fixture project the files are analysed in, relative to `tests/`
    /// (from `// project:`).
    project: Option<String>,
    /// The project's files to analyse, relative to the project (from
    /// `// file:`).
    project_files: Vec<String>,
    /// The diagnostic codes the comparison covers (from `// codes:`).
    codes: Vec<String>,
    /// The diagnostics the files must report (from `// expect_diagnostic:`).
    expect_diagnostics: Vec<ExpectedDiagnostic>,
    /// The PHP version a body fixture is analysed for, against the full
    /// embedded stubs (from `// php:`).
    php_version: Option<PhpVersion>,
    /// Whether a body diagnostics fixture must report nothing at all (from
    /// `// expect_clean`).
    expect_clean: bool,
    /// Codes a body diagnostics fixture must report (from `// expect_code:`).
    expect_codes: Vec<String>,
    /// Codes a body diagnostics fixture must not report (from
    /// `// expect_absent_code:`).
    expect_absent_codes: Vec<String>,
}

/// One `// expect_diagnostic: [FILE:]LINE: message` line.
struct ExpectedDiagnostic {
    /// The analysed file it is in, when the fixture names more than one.
    file: Option<String>,
    /// 1-based line.
    line: u32,
    message: String,
    /// Why PHPantom does not report it yet (a trailing `// SKIP: reason`).
    skip: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Feature {
    Completion,
    Hover,
    Definition,
    SignatureHelp,
    Diagnostics,
    References,
}

/// A single file within a fixture (path + content).
struct FixtureFile {
    /// Relative path (e.g. `src/Service.php` or `main.php`).
    path: String,
    /// PHP source content.
    content: String,
}

/// A `// ^^^ label` line in a fixture body: the carets mark a range on the
/// closest line above that is not itself an annotation.
#[derive(Debug, Clone)]
struct CaretAnnotation {
    /// Index into `ParsedFixture::files`.
    file: usize,
    /// 0-based line the carets point at.
    line: u32,
    /// Start and end character (0-based, UTF-16 code units) on that line.
    start: u32,
    end: u32,
    /// The text after the carets.
    label: String,
}

/// Parsed fixture ready for execution.
struct ParsedFixture {
    meta: TestMeta,
    /// The files declared in the fixture body. For single-file fixtures
    /// there is exactly one.
    files: Vec<FixtureFile>,
    /// Whether the body has a `<>` cursor marker.
    has_cursor: bool,
    /// Index into `files` for the file that contains the cursor.
    cursor_file: usize,
    /// Cursor line (0-based).
    cursor_line: u32,
    /// Cursor character (0-based, UTF-16 code units).
    cursor_char: u32,
    /// Every `// ^^^` annotation in the body, in file order.
    carets: Vec<CaretAnnotation>,
}

fn parse_fixture(content: &str) -> Result<ParsedFixture, String> {
    // Split header and body on the first `---` line.
    let (header, body) = content
        .split_once("\n---\n")
        .or_else(|| content.split_once("\n---"))
        .ok_or("Fixture missing `---` separator between header and body")?;

    let meta = parse_header(header)?;
    // A diagnostics fixture with a project analyses files of that project;
    // it has no cursor and no body.
    if meta.feature == Feature::Diagnostics && meta.project.is_some() {
        return Ok(ParsedFixture {
            meta,
            files: Vec::new(),
            has_cursor: false,
            cursor_file: 0,
            cursor_line: 0,
            cursor_char: 0,
            carets: Vec::new(),
        });
    }

    let mut fixture = parse_body(body.trim_start(), meta)?;
    let cursor_optional = matches!(
        fixture.meta.feature,
        Feature::Diagnostics | Feature::References
    );
    if !fixture.has_cursor && !cursor_optional {
        return Err("Fixture body missing `<>` cursor marker".to_string());
    }
    let meta = &fixture.meta;
    if meta.feature == Feature::Diagnostics
        && !meta.expect_clean
        && meta.expect_codes.is_empty()
        && meta.expect_absent_codes.is_empty()
        && fixture.carets.is_empty()
    {
        return Err(
            "Diagnostics fixture needs either a `// project:` and a `// file:`, \
             or a body with `// expect_clean`, `// expect_code:`, `// expect_absent_code:` \
             or `// ^^^` annotations"
                .to_string(),
        );
    }
    if meta.feature == Feature::References && !fixture.has_cursor {
        // Without a cursor, the request fires on the first declaration.
        let def = fixture
            .carets
            .iter()
            .find(|c| c.label == "def")
            .ok_or("References fixture needs a `<>` cursor or a `// ^^^ def` annotation")?;
        fixture.cursor_file = def.file;
        fixture.cursor_line = def.line;
        fixture.cursor_char = def.start;
        fixture.has_cursor = true;
    }
    Ok(fixture)
}

fn parse_header(header: &str) -> Result<TestMeta, String> {
    let mut description = String::new();
    let mut feature = None;
    let mut expect = Vec::new();
    let mut expect_absent = Vec::new();
    let mut expect_hover = Vec::new();
    let mut expect_definition = Vec::new();
    let mut expect_sig_label = None;
    let mut expect_sig_active = None;
    let mut expect_sig_params = Vec::new();
    let mut ignore = None;
    let mut project = None;
    let mut project_files = Vec::new();
    let mut codes = Vec::new();
    let mut expect_diagnostics = Vec::new();
    let mut php_version = None;
    let mut expect_clean = false;
    let mut expect_codes = Vec::new();
    let mut expect_absent_codes = Vec::new();

    for line in header.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("// test:") {
            description = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("// feature:") {
            feature = Some(match val.trim() {
                "completion" => Feature::Completion,
                "hover" => Feature::Hover,
                "definition" => Feature::Definition,
                "signature_help" => Feature::SignatureHelp,
                "diagnostics" => Feature::Diagnostics,
                "references" => Feature::References,
                other => return Err(format!("Unknown feature: {other}")),
            });
        } else if let Some(val) = line.strip_prefix("// expect:") {
            expect.push(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// expect_absent:") {
            expect_absent.push(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// expect_hover:") {
            // Format: `symbol => substring`
            if let Some((sym, sub)) = val.split_once("=>") {
                expect_hover.push((sym.trim().to_string(), sub.trim().to_string()));
            } else {
                return Err(format!("Invalid expect_hover format: {val}"));
            }
        } else if let Some(val) = line.strip_prefix("// expect_definition:") {
            expect_definition.push(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// expect_sig_label:") {
            expect_sig_label = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// expect_sig_active:") {
            expect_sig_active = Some(
                val.trim()
                    .parse::<u32>()
                    .map_err(|e| format!("Invalid expect_sig_active: {e}"))?,
            );
        } else if let Some(val) = line.strip_prefix("// expect_sig_param:") {
            expect_sig_params.push(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// ignore:") {
            ignore = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// project:") {
            project = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// file:") {
            project_files.push(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// codes:") {
            codes.extend(
                val.split(',')
                    .map(str::trim)
                    .filter(|code| !code.is_empty())
                    .map(str::to_string),
            );
        } else if let Some(val) = line.strip_prefix("// expect_diagnostic:") {
            expect_diagnostics.push(parse_expected_diagnostic(val)?);
        } else if let Some(val) = line.strip_prefix("// php:") {
            php_version = Some(
                PhpVersion::from_composer_constraint(val.trim())
                    .ok_or_else(|| format!("Invalid php version: {val}"))?,
            );
        } else if line == "// expect_clean" {
            expect_clean = true;
        } else if let Some(val) = line.strip_prefix("// expect_code:") {
            expect_codes.push(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("// expect_absent_code:") {
            expect_absent_codes.push(val.trim().to_string());
        }
        // Lines that don't match any prefix are silently ignored (comments).
    }

    let feature = feature.ok_or("Fixture missing `// feature:` header")?;
    if description.is_empty() {
        return Err("Fixture missing `// test:` header".to_string());
    }
    if feature == Feature::Diagnostics && project.is_some() {
        let body_checks =
            expect_clean || !expect_codes.is_empty() || !expect_absent_codes.is_empty();
        if body_checks || project_files.is_empty() {
            return Err(
                "A project diagnostics fixture needs a `// file:` and checks \
                 only `// expect_diagnostic:` lines"
                    .to_string(),
            );
        }
    }

    Ok(TestMeta {
        description,
        feature,
        expect,
        expect_absent,
        expect_hover,
        expect_definition,
        expect_sig_label,
        expect_sig_active,
        expect_sig_params,
        ignore,
        project,
        project_files,
        codes,
        expect_diagnostics,
        php_version,
        expect_clean,
        expect_codes,
        expect_absent_codes,
    })
}

/// Parse the value of an `// expect_diagnostic:` line:
/// `[FILE:]LINE: message`, optionally followed by `// SKIP: reason`.
/// `FILE` is one of the fixture's `// file:` paths, and defaults to the
/// first.
fn parse_expected_diagnostic(val: &str) -> Result<ExpectedDiagnostic, String> {
    let (val, skip) = match val.split_once("// SKIP:") {
        Some((val, reason)) => (val, Some(reason.trim().to_string())),
        None => (val, None),
    };
    let (location, message) = val
        .trim()
        .split_once(": ")
        .ok_or_else(|| format!("Invalid expect_diagnostic format: {val}"))?;
    let (file, line) = match location.rsplit_once(':') {
        Some((file, line)) => (Some(file.to_string()), line),
        None => (None, location),
    };
    let line = line
        .parse()
        .map_err(|e| format!("Invalid line in expect_diagnostic `{val}`: {e}"))?;
    Ok(ExpectedDiagnostic {
        file,
        line,
        // A message spanning lines is written with `\n`.
        message: message.trim().replace("\\n", "\n"),
        skip,
    })
}

fn parse_body(body: &str, meta: TestMeta) -> Result<ParsedFixture, String> {
    // Check for multi-file format: lines starting with `=== path ===`.
    let raw_files = if body.contains("\n=== ") || body.starts_with("=== ") {
        split_multi_file_body(body)?
    } else {
        vec![("test.php".to_string(), body.to_string())]
    };

    let mut fixture = ParsedFixture {
        meta,
        files: Vec::with_capacity(raw_files.len()),
        has_cursor: false,
        cursor_file: 0,
        cursor_line: 0,
        cursor_char: 0,
        carets: Vec::new(),
    };
    for (index, (path, raw)) in raw_files.into_iter().enumerate() {
        // Carets are written against the text as it reads, `<>` included,
        // so they are read before the marker is stripped.
        let carets = parse_carets(&raw);
        let (content, marker) = match strip_cursor(&raw) {
            Some((content, line, char_index)) => {
                if fixture.has_cursor {
                    return Err("More than one `<>` cursor marker".to_string());
                }
                fixture.has_cursor = true;
                fixture.cursor_file = index;
                fixture.cursor_line = line;
                fixture.cursor_char = utf16_column(nth_line(&content, line), char_index);
                (content, Some((line, char_index)))
            }
            None => (raw, None),
        };
        for (line, mut start, mut end, label) in carets {
            if let Some((marker_line, marker_char)) = marker
                && marker_line == line
            {
                if start > marker_char {
                    start -= 2;
                }
                if end > marker_char {
                    end -= 2;
                }
            }
            let text = nth_line(&content, line);
            fixture.carets.push(CaretAnnotation {
                file: index,
                line,
                start: utf16_column(text, start),
                end: utf16_column(text, end),
                label,
            });
        }
        fixture.files.push(FixtureFile { path, content });
    }
    Ok(fixture)
}

/// Split a `=== path ===` body into its files, as paths and contents.
fn split_multi_file_body(body: &str) -> Result<Vec<(String, String)>, String> {
    let mut files: Vec<(String, String)> = Vec::new();
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("=== ")
            && let Some(path_str) = rest.strip_suffix(" ===")
        {
            files.push((path_str.trim().to_string(), String::new()));
            continue;
        }
        if let Some((_, content)) = files.last_mut() {
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(line);
        }
    }
    if files.is_empty() {
        return Err("Multi-file fixture has no files".to_string());
    }
    Ok(files)
}

/// Strip the `<>` cursor marker from PHP source and return the cleaned
/// content along with the marker's line and character index (in `char`s).
fn strip_cursor(source: &str) -> Option<(String, u32, usize)> {
    let marker_pos = source.find("<>")?;

    let before = &source[..marker_pos];
    let line = before.chars().filter(|&c| c == '\n').count() as u32;
    let last_newline = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let char_index = before[last_newline..].chars().count();

    let mut cleaned = String::with_capacity(source.len() - 2);
    cleaned.push_str(before);
    cleaned.push_str(&source[marker_pos + 2..]);

    Some((cleaned, line, char_index))
}

fn nth_line(content: &str, line: u32) -> &str {
    content.lines().nth(line as usize).unwrap_or("")
}

/// The UTF-16 column of the `char_index`-th character of `line`.
fn utf16_column(line: &str, char_index: usize) -> u32 {
    let width: usize = line.chars().take(char_index).map(char::len_utf16).sum();
    // Past the end of the line, each missing character counts as one.
    let missing = char_index.saturating_sub(line.chars().count());
    (width + missing) as u32
}

/// Read every `// ^^^ label` line of a file.  The carets mark the columns
/// (counted in characters) of the closest line above that is not itself an
/// annotation, so several annotations can stack under one line.  Returns
/// `(line, start, end, label)` with `end` exclusive.
fn parse_carets(content: &str) -> Vec<(u32, usize, usize, String)> {
    let mut carets = Vec::new();
    let mut target: Option<u32> = None;
    for (index, line) in content.lines().enumerate() {
        let Some(caret) = parse_caret_line(line) else {
            target = Some(index as u32);
            continue;
        };
        if let Some(target) = target {
            let (start, end, label) = caret;
            carets.push((target, start, end, label));
        }
    }
    carets
}

fn parse_caret_line(line: &str) -> Option<(usize, usize, String)> {
    let rest = line.trim_start().strip_prefix("//")?;
    if !rest.trim_start().starts_with('^') {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let start = chars.iter().position(|&c| c == '^')?;
    let len = chars[start..].iter().take_while(|&&c| c == '^').count();
    let label: String = chars[start + len..].iter().collect();
    Some((start, start + len, label.trim().to_string()))
}

// ─── Test execution ─────────────────────────────────────────────────────────

fn run_fixture(path: &Path, content: String) -> datatest_stable::Result<()> {
    let fixture =
        parse_fixture(&content).map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    // Handle ignored fixtures.
    if let Some(reason) = &fixture.meta.ignore {
        eprintln!("IGNORED: {reason}");
        return Ok(());
    }

    // The project backend runs its own startup, so this one runs outside
    // the runtime below.
    if fixture.meta.feature == Feature::Diagnostics && fixture.meta.project.is_some() {
        return run_diagnostics(&fixture).map_err(|e| format!("{} — {e}", path.display()).into());
    }

    // Use a single-threaded tokio runtime for async LSP calls.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to build tokio runtime: {e}"))?;

    rt.block_on(async {
        match fixture.meta.feature {
            Feature::Completion => run_completion(&fixture).await,
            Feature::Hover => run_hover(&fixture).await,
            Feature::Definition => run_definition(&fixture).await,
            Feature::SignatureHelp => run_signature_help(&fixture).await,
            Feature::Diagnostics => run_body_diagnostics(&fixture).await,
            Feature::References => run_references(&fixture).await,
        }
    })
    .map_err(|e| {
        // The harness prints the error escaped onto one line, which makes
        // the suggested `// ^^^` annotations unreadable.
        eprintln!("{e}");
        format!("{} — {e}", path.display())
    })?;

    Ok(())
}

/// Build a URI for a fixture file path.
fn file_uri(path: &str) -> Uri {
    Uri::parse(&format!("file:///{path}"))
        .unwrap_or_else(|_| Uri::parse("file:///test.php").unwrap())
}

/// Open all fixture files on the backend via the public `did_open` LSP method.
/// Returns the URI of the cursor file.
async fn open_files(backend: &Backend, fixture: &ParsedFixture) -> Uri {
    let mut cursor_uri = None;

    for (i, file) in fixture.files.iter().enumerate() {
        let uri = file_uri(&file.path);

        let open_params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string().into(),
                version: 0,
                text: file.content.clone(),
            },
        };
        backend.did_open(open_params).await;

        if i == fixture.cursor_file {
            cursor_uri = Some(uri);
        }
    }

    cursor_uri.expect("cursor file not opened")
}

// ─── Feature runners ────────────────────────────────────────────────────────

async fn run_completion(fixture: &ParsedFixture) -> Result<(), String> {
    let backend = fixture_backend(&fixture.meta);
    let uri = open_files(&backend, fixture).await;

    let position = Position {
        line: fixture.cursor_line,
        character: fixture.cursor_char,
    };

    let params = CompletionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    let response = backend
        .completion(params)
        .await
        .map_err(|e| format!("Completion request failed: {e}"))?;

    let items = match response {
        Some(CompletionResponse::CompletionItemList(items)) => items,
        Some(CompletionResponse::CompletionList(list)) => list.items,
        None => Vec::new(),
    };

    let labels: Vec<String> = items.iter().map(|item| item.label.clone()).collect();

    for expected in &fixture.meta.expect {
        let found = labels
            .iter()
            .any(|label| label.starts_with(expected.as_str()));
        if !found {
            return Err(format!(
                "Expected completion label starting with `{expected}` not found.\nGot: {labels:?}"
            ));
        }
    }

    for absent in &fixture.meta.expect_absent {
        let found = labels
            .iter()
            .any(|label| label.starts_with(absent.as_str()));
        if found {
            return Err(format!(
                "Completion label starting with `{absent}` should NOT be present.\nGot: {labels:?}"
            ));
        }
    }

    if fixture.meta.expect.is_empty() && fixture.meta.expect_absent.is_empty() {
        return Err(
            "Completion fixture has no `// expect:` or `// expect_absent:` assertions".to_string(),
        );
    }

    Ok(())
}

/// Extract the text content from a Hover response.
fn extract_hover_text(hover: &Hover) -> String {
    match &hover.contents {
        Contents::MarkupContent(mc) => mc.value.clone(),
        other => panic!("expected a markup hover, got {other:?}"),
    }
}

async fn run_hover(fixture: &ParsedFixture) -> Result<(), String> {
    let backend = fixture_backend(&fixture.meta);
    let uri = open_files(&backend, fixture).await;
    let source = &fixture.files[fixture.cursor_file].content;

    // If there are explicit expect_hover assertions (symbol => substring),
    // find each symbol in the source and hover over it.
    if !fixture.meta.expect_hover.is_empty() {
        for (symbol, expected_substring) in &fixture.meta.expect_hover {
            let offset = source.find(symbol.as_str()).ok_or_else(|| {
                format!("Symbol `{symbol}` not found in fixture source for hover assertion")
            })?;

            let before = &source[..offset];
            let line = before.chars().filter(|&c| c == '\n').count() as u32;
            let last_nl = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let character = before[last_nl..].len() as u32;

            let params = HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position { line, character },
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            };

            let hover = backend
                .hover(params)
                .await
                .map_err(|e| format!("Hover request failed: {e}"))?;

            match hover {
                Some(h) => {
                    let hover_text = extract_hover_text(&h);
                    if !hover_text.contains(expected_substring.as_str()) {
                        return Err(format!(
                            "Hover on `{symbol}` expected to contain `{expected_substring}`, got:\n{hover_text}"
                        ));
                    }
                    check_hover_absent(fixture, &hover_text)?;
                }
                None => {
                    return Err(format!("No hover result for symbol `{symbol}`"));
                }
            }
        }
        return Ok(());
    }

    // Fallback: hover at the cursor position and check expect lines as substrings.
    let position = Position {
        line: fixture.cursor_line,
        character: fixture.cursor_char,
    };

    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };

    let hover = backend
        .hover(params)
        .await
        .map_err(|e| format!("Hover request failed: {e}"))?;

    match hover {
        Some(h) => {
            let hover_text = extract_hover_text(&h);
            for expected in &fixture.meta.expect {
                if !hover_text.contains(expected.as_str()) {
                    return Err(format!(
                        "Hover expected to contain `{expected}`, got:\n{hover_text}"
                    ));
                }
            }
            check_hover_absent(fixture, &hover_text)?;
        }
        None => {
            if !fixture.meta.expect.is_empty() {
                return Err("No hover result at cursor position".to_string());
            }
        }
    }

    Ok(())
}

/// Verify none of the fixture's `// expect_absent:` substrings appear in the
/// hover text.  This is what lets a hover fixture assert that a type was
/// *dropped* (e.g. a variable keeping its pre-branch type past a dead
/// branch), not merely that the expected type is somewhere in the union.
fn check_hover_absent(fixture: &ParsedFixture, hover_text: &str) -> Result<(), String> {
    for absent in &fixture.meta.expect_absent {
        if hover_text.contains(absent.as_str()) {
            return Err(format!(
                "Hover should NOT contain `{absent}`, got:\n{hover_text}"
            ));
        }
    }
    Ok(())
}

async fn run_definition(fixture: &ParsedFixture) -> Result<(), String> {
    let backend = fixture_backend(&fixture.meta);
    let uri = open_files(&backend, fixture).await;

    let position = Position {
        line: fixture.cursor_line,
        character: fixture.cursor_char,
    };

    let params = DefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let result = backend
        .goto_definition(params)
        .await
        .map_err(|e| format!("Goto definition request failed: {e}"))?;

    if fixture.meta.expect_definition.is_empty() {
        return Err("Definition fixture has no `// expect_definition:` assertions".to_string());
    }

    let locations = match result {
        Some(DefinitionResponse::Definition(Definition::Location(loc))) => vec![loc],
        Some(DefinitionResponse::Definition(Definition::LocationList(locs))) => locs,
        Some(DefinitionResponse::DefinitionLinkList(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
        None => Vec::new(),
    };

    for expected in &fixture.meta.expect_definition {
        if expected.starts_with("self:") {
            // `self:LINE` — definition is in the cursor file at the given line (1-based).
            let expected_line: u32 = expected
                .strip_prefix("self:")
                .unwrap()
                .trim()
                .parse()
                .map_err(|e| format!("Invalid line in expect_definition: {e}"))?;

            let found = locations.iter().any(|loc| {
                loc.uri == uri && loc.range.start.line == expected_line.saturating_sub(1)
            });
            if !found {
                let actual: Vec<String> = locations
                    .iter()
                    .map(|l| format!("{}:{}", l.uri.path(), l.range.start.line + 1))
                    .collect();
                return Err(format!(
                    "Expected definition at self:{expected_line}, got: {actual:?}"
                ));
            }
        } else {
            // General `file:line` format.
            let (file, line_str) = expected
                .rsplit_once(':')
                .ok_or_else(|| format!("Invalid expect_definition format: {expected}"))?;
            let expected_line: u32 = line_str
                .trim()
                .parse()
                .map_err(|e| format!("Invalid line in expect_definition: {e}"))?;

            let found = locations.iter().any(|loc| {
                loc.uri.path().ends_with(file)
                    && loc.range.start.line == expected_line.saturating_sub(1)
            });
            if !found {
                let actual: Vec<String> = locations
                    .iter()
                    .map(|l| format!("{}:{}", l.uri.path(), l.range.start.line + 1))
                    .collect();
                return Err(format!(
                    "Expected definition at {expected}, got: {actual:?}"
                ));
            }
        }
    }

    Ok(())
}

async fn run_signature_help(fixture: &ParsedFixture) -> Result<(), String> {
    let backend = fixture_backend(&fixture.meta);
    let uri = open_files(&backend, fixture).await;

    let position = Position {
        line: fixture.cursor_line,
        character: fixture.cursor_char,
    };

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        context: None,
    };

    let result = backend
        .signature_help(params)
        .await
        .map_err(|e| format!("Signature help request failed: {e}"))?;

    let has_assertions = fixture.meta.expect_sig_label.is_some()
        || fixture.meta.expect_sig_active.is_some()
        || !fixture.meta.expect_sig_params.is_empty();

    if !has_assertions {
        return Err(
            "Signature help fixture has no `// expect_sig_label:`, `// expect_sig_active:`, or `// expect_sig_param:` assertions".to_string(),
        );
    }

    let sh = result.ok_or("No signature help result at cursor position")?;
    let sig = sh
        .signatures
        .first()
        .ok_or("Signature help returned no signatures")?;

    if let Some(expected_label) = &fixture.meta.expect_sig_label
        && sig.label != *expected_label
    {
        return Err(format!(
            "Expected signature label `{expected_label}`, got `{}`",
            sig.label
        ));
    }

    if let Some(expected_active) = fixture.meta.expect_sig_active {
        let actual_active = match sh.active_parameter {
            Some(ActiveParameter::Int(n)) => n,
            Some(ActiveParameter::Null) | None => 0,
        };
        if actual_active != expected_active {
            return Err(format!(
                "Expected active parameter {expected_active}, got {actual_active}"
            ));
        }
    }

    if !fixture.meta.expect_sig_params.is_empty() {
        let param_labels: Vec<String> = sig
            .parameters
            .as_ref()
            .map(|params| {
                params
                    .iter()
                    .map(|p| match &p.label {
                        ParameterInformationLabel::Tuple((start, end)) => {
                            sig.label[*start as usize..*end as usize].to_string()
                        }
                        ParameterInformationLabel::String(s) => s.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (i, expected) in fixture.meta.expect_sig_params.iter().enumerate() {
            if i >= param_labels.len() {
                return Err(format!(
                    "Expected parameter {i} to be `{expected}`, but only {} parameters found",
                    param_labels.len()
                ));
            }
            if param_labels[i] != *expected {
                return Err(format!(
                    "Expected parameter {i} label `{expected}`, got `{}`",
                    param_labels[i]
                ));
            }
        }
    }

    Ok(())
}

/// Analyse the fixture's body and check the codes of every diagnostic it
/// reports against `// expect_clean`, `// expect_code:` and
/// `// expect_absent_code:`.
async fn run_body_diagnostics(fixture: &ParsedFixture) -> Result<(), String> {
    let backend = fixture_backend(&fixture.meta);
    let uri = open_files(&backend, fixture).await.to_string();
    let content = &fixture.files[fixture.cursor_file].content;
    let mut diagnostics = Vec::new();
    backend.collect_fast_diagnostics(&uri, content, &mut diagnostics);
    backend.collect_slow_diagnostics(&uri, content, &mut diagnostics);
    let reported: Vec<String> = diagnostics
        .iter()
        .map(|diagnostic| {
            let code = match &diagnostic.code {
                Some(Code::String(code)) => code.as_str(),
                _ => "",
            };
            let message = match &diagnostic.message {
                Message::String(text) => text.as_str(),
                Message::MarkupContent(markup) => markup.value.as_str(),
            };
            format!("{}: {code}: {message}", diagnostic.range.start.line + 1)
        })
        .collect();
    let has_code = |code: &str| {
        diagnostics
            .iter()
            .any(|d| matches!(&d.code, Some(Code::String(c)) if c == code))
    };

    let meta = &fixture.meta;
    let mut failures = check_diagnostic_carets(fixture, content, &diagnostics);
    if meta.expect_clean && !reported.is_empty() {
        failures.push("expected no diagnostics".to_string());
    }
    for code in &meta.expect_codes {
        if !has_code(code) {
            failures.push(format!("missing {code}"));
        }
    }
    for code in &meta.expect_absent_codes {
        if has_code(code) {
            failures.push(format!("unexpected {code}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("{}\nGot: {reported:#?}", failures.join("\n")))
    }
}

/// Compare the diagnostics against the body's `// ^^^ code[: message]`
/// annotations.  When there are any, they are the whole answer: each must
/// match a diagnostic's range and code (and its message, when given), and
/// every diagnostic must be annotated.  An annotation ending in
/// `// SKIP: reason` is one PHPantom does not report yet: it is not
/// required, and a diagnostic with its range is not held against the
/// fixture.
fn check_diagnostic_carets(
    fixture: &ParsedFixture,
    content: &str,
    diagnostics: &[Diagnostic],
) -> Vec<String> {
    struct Expected<'a> {
        caret: &'a CaretAnnotation,
        code: &'a str,
        message: Option<&'a str>,
        skip: bool,
    }
    if fixture.carets.is_empty() {
        return Vec::new();
    }
    let expected: Vec<Expected> = fixture
        .carets
        .iter()
        .map(|caret| {
            let (label, skip) = match caret.label.split_once("// SKIP") {
                Some((label, _)) => (label.trim(), true),
                None => (caret.label.as_str(), false),
            };
            let (code, message) = match label.split_once(':') {
                Some((code, message)) => (code.trim(), Some(message.trim())),
                None => (label.trim(), None),
            };
            Expected {
                caret,
                code,
                message,
                skip,
            }
        })
        .collect();
    let code_of = |d: &Diagnostic| match &d.code {
        Some(Code::String(code)) => code.clone(),
        Some(Code::Int(code)) => code.to_string(),
        None => String::new(),
    };
    let message_of = |d: &Diagnostic| match &d.message {
        Message::String(text) => text.clone(),
        Message::MarkupContent(markup) => markup.value.clone(),
    };
    let at = |caret: &CaretAnnotation, d: &Diagnostic| {
        d.range.start.line == caret.line
            && d.range.end.line == caret.line
            && d.range.start.character == caret.start
            && d.range.end.character == caret.end
    };
    let matches = |e: &Expected, d: &Diagnostic| {
        at(e.caret, d) && code_of(d) == e.code && e.message.is_none_or(|m| message_of(d) == m)
    };

    let mut failures = Vec::new();
    for e in expected.iter().filter(|e| !e.skip) {
        if !diagnostics.iter().any(|d| matches(e, d)) {
            failures.push(format!(
                "missing at {}:{}-{}: {}",
                e.caret.line + 1,
                e.caret.start,
                e.caret.end,
                e.caret.label
            ));
        }
    }
    for d in diagnostics {
        let accounted = expected.iter().any(|e| {
            if e.skip {
                at(e.caret, d)
            } else {
                matches(e, d)
            }
        });
        if !accounted {
            failures.push(format!(
                "unexpected, annotate it as:\n{}",
                render_diagnostic_caret(content, d, &code_of(d), &message_of(d))
            ));
        }
    }
    failures
}

/// Render a diagnostic the way a fixture annotates it: the line it starts
/// on, then a `// ^^^ code: message` line under it.
fn render_diagnostic_caret(content: &str, d: &Diagnostic, code: &str, message: &str) -> String {
    let text = nth_line(content, d.range.start.line);
    let utf16_to_char = |column: u32| {
        let mut width = 0;
        for (index, c) in text.chars().enumerate() {
            if width >= column as usize {
                return index;
            }
            width += c.len_utf16();
        }
        text.chars().count()
    };
    let start = utf16_to_char(d.range.start.character);
    let end = if d.range.end.line == d.range.start.line {
        utf16_to_char(d.range.end.character).max(start + 1)
    } else {
        text.chars().count().max(start + 1)
    };
    let mut note = String::new();
    if d.range.end.line != d.range.start.line {
        note = format!(
            " (spans to line {}, cannot be annotated)",
            d.range.end.line + 1
        );
    } else if start < 2 {
        note = " (starts before column 2, cannot be annotated)".to_string();
    }
    // `//` sits at the line's indentation, or as far right as it fits.
    let indent = text
        .chars()
        .take_while(|c| c.is_whitespace())
        .count()
        .min(start.saturating_sub(2));
    let gap = start.saturating_sub(indent + 2);
    format!(
        "  {line}: {text}\n  {line}: {}//{}{} {code}: {message}{note}",
        " ".repeat(indent),
        " ".repeat(gap),
        "^".repeat(end - start),
        line = d.range.start.line + 1,
    )
}

async fn run_references(fixture: &ParsedFixture) -> Result<(), String> {
    let backend = fixture_backend(&fixture.meta);
    let uri = open_files(&backend, fixture).await;
    let uris: Vec<Uri> = fixture.files.iter().map(|f| file_uri(&f.path)).collect();

    let request = |include_declaration: bool| ReferenceParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position {
                line: fixture.cursor_line,
                character: fixture.cursor_char,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: ReferenceContext {
            include_declaration,
        },
    };
    let render = |file: &str, line: u32, start: u32, end: u32| {
        format!("{file}:{}:{}-{}", line + 1, start, end)
    };
    let path_of = |location: &Location| {
        uris.iter()
            .position(|u| *u == location.uri)
            .map(|i| fixture.files[i].path.clone())
            .unwrap_or_else(|| location.uri.to_string())
    };

    for include_declaration in [true, false] {
        let mut expected: Vec<String> = fixture
            .carets
            .iter()
            .filter(|c| c.label == "ref" || (include_declaration && c.label == "def"))
            .map(|c| render(&fixture.files[c.file].path, c.line, c.start, c.end))
            .collect();
        if let Some(c) = fixture
            .carets
            .iter()
            .find(|c| c.label != "ref" && c.label != "def")
        {
            return Err(format!(
                "Unknown references annotation `{}` (use `def` or `ref`)",
                c.label
            ));
        }
        let mut actual: Vec<String> = backend
            .references(request(include_declaration))
            .await
            .map_err(|e| format!("References request failed: {e}"))?
            .unwrap_or_default()
            .iter()
            .map(|l| {
                render(
                    &path_of(l),
                    l.range.start.line,
                    l.range.start.character,
                    l.range.end.character,
                )
            })
            .collect();
        expected.sort();
        actual.sort();
        if expected != actual {
            return Err(format!(
                "References (include_declaration: {include_declaration}) differ.\n\
                 Expected: {expected:#?}\nGot: {actual:#?}"
            ));
        }
    }
    Ok(())
}

/// Analyse the fixture's project files and compare the diagnostics whose
/// code the fixture names with the ones it expects, line and message.
///
/// An expectation marked `// SKIP` is one PHPantom does not report yet: it
/// is not required, and a diagnostic on its line is not held against the
/// fixture.
fn run_diagnostics(fixture: &ParsedFixture) -> Result<(), String> {
    let meta = &fixture.meta;
    let project = meta.project.as_deref().expect("checked when parsed");
    let root = fixture_project::absolute_path(&Path::new("tests").join(project));
    let backend = fixture_project::project_backend(&root);

    let mut reported: Vec<(String, u32, String)> = Vec::new();
    for file in &meta.project_files {
        let path = root.join(file);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        let uri = Uri::from_file_path(&path)
            .map_err(|_| format!("{} has no file URI", path.display()))?
            .to_string();
        backend.update_ast(&uri, &content);
        let mut diagnostics = Vec::new();
        backend.collect_slow_diagnostics(&uri, &content, &mut diagnostics);
        for diagnostic in diagnostics {
            let code = match &diagnostic.code {
                Some(Code::String(code)) => code.as_str(),
                _ => continue,
            };
            if !meta.codes.iter().any(|wanted| wanted == code) {
                continue;
            }
            let message = match &diagnostic.message {
                Message::String(text) => text.clone(),
                Message::MarkupContent(markup) => markup.value.clone(),
            };
            reported.push((file.clone(), diagnostic.range.start.line + 1, message));
        }
    }

    // An expectation without a file is in the first one the fixture names.
    let file_of = |expected: &ExpectedDiagnostic| -> String {
        expected
            .file
            .clone()
            .unwrap_or_else(|| meta.project_files[0].clone())
    };
    let mut failures = Vec::new();
    for expected in meta.expect_diagnostics.iter().filter(|e| e.skip.is_none()) {
        let file = file_of(expected);
        let found = reported.iter().any(|(f, line, message)| {
            *f == file && *line == expected.line && *message == expected.message
        });
        if !found {
            failures.push(format!(
                "missing {file}:{}: {}",
                expected.line, expected.message
            ));
        }
    }
    for (file, line, message) in &reported {
        let expected = meta.expect_diagnostics.iter().any(|e| {
            file_of(e) == *file && e.line == *line && (e.skip.is_some() || e.message == *message)
        });
        if !expected {
            failures.push(format!("unexpected {file}:{line}: {message}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

// ─── Harness ────────────────────────────────────────────────────────────────

datatest_stable::harness! {
    { test = run_fixture, root = "tests/fixtures", pattern = r"\.fixture$" },
}
