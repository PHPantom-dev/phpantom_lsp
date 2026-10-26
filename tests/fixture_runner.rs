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

/// Create a test backend with embedded class and function stubs pre-loaded.
fn create_fixture_backend() -> Backend {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("UnitEnum", UNIT_ENUM_STUB);
    class_stubs.insert("BackedEnum", BACKED_ENUM_STUB);

    let mut func_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    func_stubs.insert("array_reduce", ARRAY_FUNC_STUB);
    func_stubs.insert("array_sum", ARRAY_FUNC_STUB);
    func_stubs.insert("array_product", ARRAY_FUNC_STUB);
    func_stubs.insert("array_map", ARRAY_FUNC_STUB);
    func_stubs.insert("array_filter", ARRAY_FUNC_STUB);

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
}

/// A single file within a fixture (path + content).
struct FixtureFile {
    /// Relative path (e.g. `src/Service.php` or `main.php`).
    path: String,
    /// PHP source content.
    content: String,
}

/// Parsed fixture ready for execution.
struct ParsedFixture {
    meta: TestMeta,
    /// The files declared in the fixture body. The file containing the `<>`
    /// cursor is always present. For single-file fixtures there is exactly one.
    files: Vec<FixtureFile>,
    /// Index into `files` for the file that contains the cursor.
    cursor_file: usize,
    /// Cursor line (0-based).
    cursor_line: u32,
    /// Cursor character (0-based, UTF-16 code units).
    cursor_char: u32,
}

fn parse_fixture(content: &str) -> Result<ParsedFixture, String> {
    // Split header and body on the first `---` line.
    let (header, body) = content
        .split_once("\n---\n")
        .or_else(|| content.split_once("\n---"))
        .ok_or("Fixture missing `---` separator between header and body")?;

    let meta = parse_header(header)?;
    // A diagnostics fixture analyses files of its project; it has no
    // cursor and no body.
    let (files, cursor_file, cursor_line, cursor_char) = if meta.feature == Feature::Diagnostics {
        (Vec::new(), 0, 0, 0)
    } else {
        parse_body(body.trim_start())?
    };

    Ok(ParsedFixture {
        meta,
        files,
        cursor_file,
        cursor_line,
        cursor_char,
    })
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
        }
        // Lines that don't match any prefix are silently ignored (comments).
    }

    let feature = feature.ok_or("Fixture missing `// feature:` header")?;
    if description.is_empty() {
        return Err("Fixture missing `// test:` header".to_string());
    }
    if feature == Feature::Diagnostics && (project.is_none() || project_files.is_empty()) {
        return Err("Diagnostics fixture needs a `// project:` and a `// file:`".to_string());
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

fn parse_body(body: &str) -> Result<(Vec<FixtureFile>, usize, u32, u32), String> {
    // Check for multi-file format: lines starting with `=== path ===`.
    if body.contains("\n=== ") || body.starts_with("=== ") {
        parse_multi_file_body(body)
    } else {
        parse_single_file_body(body)
    }
}

fn parse_single_file_body(body: &str) -> Result<(Vec<FixtureFile>, usize, u32, u32), String> {
    let (content, line, char) = strip_cursor(body)?;
    Ok((
        vec![FixtureFile {
            path: "test.php".to_string(),
            content,
        }],
        0,
        line,
        char,
    ))
}

fn parse_multi_file_body(body: &str) -> Result<(Vec<FixtureFile>, usize, u32, u32), String> {
    let mut files = Vec::new();
    let mut cursor_file = None;
    let mut cursor_line = 0u32;
    let mut cursor_char = 0u32;

    let mut current_path: Option<String> = None;
    let mut current_content = String::new();

    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("=== ")
            && let Some(path_str) = rest.strip_suffix(" ===")
        {
            // Flush previous file if any.
            if let Some(path) = current_path.take() {
                let content = std::mem::take(&mut current_content);
                if content.contains("<>") {
                    let (stripped, cl, cc) = strip_cursor(&content)?;
                    cursor_file = Some(files.len());
                    cursor_line = cl;
                    cursor_char = cc;
                    files.push(FixtureFile {
                        path,
                        content: stripped,
                    });
                } else {
                    files.push(FixtureFile { path, content });
                }
            }
            current_path = Some(path_str.trim().to_string());
            continue;
        }
        if current_path.is_some() {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }

    // Flush last file.
    if let Some(path) = current_path {
        let content = std::mem::take(&mut current_content);
        if content.contains("<>") {
            let (stripped, cl, cc) = strip_cursor(&content)?;
            cursor_file = Some(files.len());
            cursor_line = cl;
            cursor_char = cc;
            files.push(FixtureFile {
                path,
                content: stripped,
            });
        } else {
            files.push(FixtureFile { path, content });
        }
    }

    if files.is_empty() {
        return Err("Multi-file fixture has no files".to_string());
    }
    let cursor_file = cursor_file.ok_or("No file contains a `<>` cursor marker")?;

    Ok((files, cursor_file, cursor_line, cursor_char))
}

/// Strip the `<>` cursor marker from PHP source and return the cleaned
/// content along with the cursor's line and character position.
fn strip_cursor(source: &str) -> Result<(String, u32, u32), String> {
    let marker_pos = source
        .find("<>")
        .ok_or("Fixture body missing `<>` cursor marker")?;

    let before = &source[..marker_pos];
    let line = before.chars().filter(|&c| c == '\n').count() as u32;
    let last_newline = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let char = before[last_newline..].len() as u32;

    let mut cleaned = String::with_capacity(source.len() - 2);
    cleaned.push_str(before);
    cleaned.push_str(&source[marker_pos + 2..]);

    Ok((cleaned, line, char))
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
    if fixture.meta.feature == Feature::Diagnostics {
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
            Feature::Diagnostics => unreachable!("run above"),
        }
    })
    .map_err(|e| format!("{} — {e}", path.display()))?;

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
    let backend = create_fixture_backend();
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
    let backend = create_fixture_backend();
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
    let backend = create_fixture_backend();
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
    let backend = create_fixture_backend();
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
