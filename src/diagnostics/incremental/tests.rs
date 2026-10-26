use std::sync::Arc;

use tower_lsp_server::gen_lsp_types::{Code, Diagnostic};

use super::*;

const URI: &str = "file:///project/src/Service.php";

/// A backend with `files` open and indexed, as `didOpen` leaves them.
fn backend_with(files: &[(&str, &str)]) -> Backend {
    let backend = Backend::new_test();
    for (uri, content) in files {
        open(&backend, uri, content);
    }
    backend
}

/// Put `content` in the buffer and the index, as `didChange` does.
fn open(backend: &Backend, uri: &str, content: &str) -> Arc<String> {
    let content = Arc::new(content.to_string());
    backend
        .open_files
        .write()
        .insert(uri.to_string(), Arc::clone(&content));
    backend.update_ast(uri, &content);
    content
}

/// Edit `uri` to `content` and run the slow pass from `baseline`.
fn edit(backend: &Backend, uri: &str, content: &str, baseline: Option<&SlowBaseline>) -> SlowPass {
    let content = open(backend, uri, content);
    backend.collect_slow_diagnostics_incremental(uri, &content, baseline)
}

/// What a slow pass over the whole of `content` reports, in a canonical
/// order.
fn full_pass(backend: &Backend, uri: &str, content: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri, content, &mut out);
    canonical(out)
}

fn canonical(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.sort_by(|a, b| {
        (
            a.range.start,
            a.range.end,
            crate::util::message_text(&a.message),
        )
            .cmp(&(
                b.range.start,
                b.range.end,
                crate::util::message_text(&b.message),
            ))
    });
    diagnostics
}

fn codes_and_lines(diagnostics: &[Diagnostic]) -> Vec<(String, u32)> {
    canonical(diagnostics.to_vec())
        .iter()
        .map(|d| {
            let code = match &d.code {
                Some(Code::String(code)) => code.clone(),
                _ => String::new(),
            };
            (code, d.range.start.line)
        })
        .collect()
}

/// Run a first pass over `before`, edit to `after`, and check the second
/// pass took the `expected` path and agrees with a pass over the whole of
/// `after`.
fn assert_edit(before: &str, after: &str, expected: SlowPassKind) -> Vec<Diagnostic> {
    let backend = backend_with(&[]);
    let first = edit(&backend, URI, before, None);
    assert_eq!(first.kind, SlowPassKind::Full(FullPassReason::NoBaseline));
    let baseline = first
        .baseline
        .expect("a first pass over current text is kept");

    let second = edit(&backend, URI, after, Some(&baseline));
    assert_eq!(second.kind, expected);
    let diagnostics = canonical(second.diagnostics);
    assert_eq!(diagnostics, full_pass(&backend, URI, after));
    diagnostics
}

const SERVICE: &str = r#"<?php
namespace App;

class Service
{
    public function first(): void
    {
        $this->missingOne();
    }

    public function second(): void
    {
        $this->missingTwo();
    }

    public function third(): void
    {
        $this->missingThree();
    }
}
"#;

#[test]
fn an_edit_to_one_method_rediagnoses_only_that_method() {
    let after = SERVICE.replace(
        "        $this->missingTwo();\n",
        "        $this->missingTwo();\n        $this->missingFour();\n",
    );
    let diagnostics = assert_edit(SERVICE, &after, SlowPassKind::Partial { units: 1 });

    // `third` is a line further down, and its diagnostic moved with it.
    assert_eq!(
        codes_and_lines(&diagnostics),
        vec![
            ("unknown_member".to_string(), 7),
            ("unknown_member".to_string(), 12),
            ("unknown_member".to_string(), 13),
            ("unknown_member".to_string(), 18),
        ]
    );
}

#[test]
fn a_fixed_call_disappears_and_the_rest_stays() {
    let after = SERVICE.replace("$this->missingTwo();", "$this->first();");
    let diagnostics = assert_edit(SERVICE, &after, SlowPassKind::Partial { units: 1 });
    assert_eq!(
        codes_and_lines(&diagnostics),
        vec![
            ("unknown_member".to_string(), 7),
            ("unknown_member".to_string(), 17),
        ]
    );
}

#[test]
fn edits_to_several_methods_are_rediagnosed_together() {
    let after = SERVICE
        .replace(
            "$this->missingOne();",
            "$this->first();\n        $this->nope();",
        )
        .replace("$this->missingThree();", "");
    assert_edit(SERVICE, &after, SlowPassKind::Partial { units: 2 });
}

#[test]
fn an_unchanged_file_reuses_the_previous_result() {
    assert_edit(SERVICE, SERVICE, SlowPassKind::Reused);
}

#[test]
fn an_edit_outside_the_bodies_runs_a_full_pass() {
    let signature = SERVICE.replace(
        "public function second(): void",
        "public function second(int $n): void",
    );
    assert_edit(
        SERVICE,
        &signature,
        SlowPassKind::Full(FullPassReason::OutsideBodies),
    );

    let between = SERVICE.replace("    public function third", "\n    public function third");
    assert_edit(
        SERVICE,
        &between,
        SlowPassKind::Full(FullPassReason::OutsideBodies),
    );
}

#[test]
fn an_untyped_method_called_elsewhere_runs_a_full_pass() {
    let before = r#"<?php
namespace App;

class Good { public function goodOnly(): void {} }
class Bad {}

class Service
{
    public function make()
    {
        return new Good();
    }

    public function consume(): void
    {
        $this->make()->goodOnly();
    }
}
"#;
    // `consume()` never changed, but what `make()` returns did, and with
    // it whether `goodOnly()` exists.
    let after = before.replace("return new Good();", "return new Bad();");
    let diagnostics = assert_edit(
        before,
        &after,
        SlowPassKind::Full(FullPassReason::BodyObservable),
    );
    assert_eq!(
        codes_and_lines(&diagnostics),
        vec![("unknown_member".to_string(), 15)]
    );
}

#[test]
fn an_untyped_method_nothing_calls_still_runs_a_partial_pass() {
    let before = r#"<?php
namespace App;

class Service
{
    public function make()
    {
        return 1;
    }

    public function other(): void
    {
        $this->missing();
    }
}
"#;
    let after = before.replace("return 1;", "return 'one';");
    assert_edit(before, &after, SlowPassKind::Partial { units: 1 });
}

#[test]
fn a_typed_method_called_elsewhere_still_runs_a_partial_pass() {
    let before = r#"<?php
namespace App;

class Service
{
    public function count(): int
    {
        return 1;
    }

    public function other(): void
    {
        $this->count();
        $this->missing();
    }
}
"#;
    let after = before.replace("return 1;", "return 2;");
    assert_edit(before, &after, SlowPassKind::Partial { units: 1 });
}

#[test]
fn a_method_returning_a_bare_class_it_could_refine_runs_a_full_pass() {
    // While `build()`'s untyped body is inferred for `run()`, the call to
    // `wrap()`, which declares a bare class and takes parameters, is
    // refined from `wrap()`'s body.
    let before = r#"<?php
namespace App;

class Box {}

class Service
{
    public function wrap(array $items): Box
    {
        return new Box();
    }

    public function build()
    {
        return $this->wrap([1]);
    }

    public function run(): void
    {
        $this->build()->missing();
    }
}
"#;
    let after = before.replace("return new Box();", "$x = 1;\n        return new Box();");
    assert_edit(
        before,
        &after,
        SlowPassKind::Full(FullPassReason::BodyObservable),
    );
}

#[test]
fn a_called_method_whose_body_nothing_reads_still_runs_a_partial_pass() {
    // `build()` calls `wrap()`, but nothing asks what `build()` returns, so
    // `wrap()`'s body is never read.
    let before = r#"<?php
namespace App;

class Box {}

class Service
{
    public function wrap(array $items): Box
    {
        return new Box();
    }

    public function build()
    {
        return $this->wrap([1]);
    }
}
"#;
    let after = before.replace("return new Box();", "$x = 1;\n        return new Box();");
    assert_edit(before, &after, SlowPassKind::Partial { units: 1 });
}

#[test]
fn a_body_read_through_another_file_runs_a_full_pass() {
    // `run()` asks `Relay::relay()`, in another file, what it returns, and
    // that is whatever `make()` here returns, though nothing here outside
    // `make()` itself spells its name.
    let relay = r#"<?php
namespace App;

class Relay
{
    public function relay()
    {
        return (new Source())->make();
    }
}
"#;
    let before = r#"<?php
namespace App;

class Good { public function goodOnly(): void {} }
class Bad {}

class Source
{
    public function make()
    {
        return new Good();
    }
}

class Consumer
{
    public function run(Relay $relay): void
    {
        $relay->relay()->goodOnly();
    }
}
"#;
    let backend = backend_with(&[("file:///project/src/Relay.php", relay)]);
    let baseline = edit(&backend, URI, before, None).baseline.unwrap();

    let after = before.replace("return new Good();", "return new Bad();");
    let second = edit(&backend, URI, &after, Some(&baseline));
    assert_eq!(
        second.kind,
        SlowPassKind::Full(FullPassReason::BodyObservable)
    );
    assert_eq!(
        codes_and_lines(&second.diagnostics),
        vec![("unknown_member".to_string(), 18)]
    );
}

#[test]
fn a_constructor_edit_runs_a_full_pass() {
    let before = r#"<?php
namespace App;

class Service
{
    public function __construct()
    {
        $this->missingOne();
    }

    public function other(): void
    {
        $this->missingTwo();
    }
}
"#;
    let after = before.replace("$this->missingOne();", "");
    assert_edit(
        before,
        &after,
        SlowPassKind::Full(FullPassReason::BodyObservable),
    );
}

#[test]
fn a_by_reference_function_called_elsewhere_runs_a_full_pass() {
    // What `fill()` leaves in `$value` comes from its body.
    let before = r#"<?php
namespace App;

class Good { public function goodOnly(): void {} }
class Bad {}

function fill(&$out): void
{
    $out = new Good();
}

function caller(): void
{
    fill($value);
    $value->goodOnly();
}
"#;
    let after = before.replace("$out = new Good();", "$out = new Bad();");
    let diagnostics = assert_edit(
        before,
        &after,
        SlowPassKind::Full(FullPassReason::BodyObservable),
    );
    assert_eq!(
        codes_and_lines(&diagnostics),
        vec![("unknown_member".to_string(), 14)]
    );
}

#[test]
fn a_function_declared_inside_an_edited_body_runs_a_full_pass() {
    let before = r#"<?php
namespace App;

class Service
{
    public function install(): void
    {
        function helper(): void {}
    }

    public function other(): void
    {
        helper();
    }
}
"#;
    let after = before.replace("function helper(): void {}", "");
    assert_edit(
        before,
        &after,
        SlowPassKind::Full(FullPassReason::BodyObservable),
    );
}

#[test]
fn a_declaration_read_out_of_a_body_runs_a_full_pass() {
    // The related model of a relationship comes from its body.
    let before = r#"<?php
namespace App;

class Post {}
class Comment {}

class User
{
    public function posts(): HasMany
    {
        return $this->hasMany(Post::class);
    }
}
"#;
    let after = before.replace("Post::class", "Comment::class");
    assert_edit(
        before,
        &after,
        SlowPassKind::Full(FullPassReason::DeclarationsChanged),
    );
}

#[test]
fn a_change_to_another_file_runs_a_full_pass() {
    let backend = backend_with(&[]);
    let baseline = edit(&backend, URI, SERVICE, None).baseline.unwrap();

    open(
        &backend,
        "file:///project/src/Other.php",
        "<?php\nnamespace App;\nclass Other {}\n",
    );
    let after = SERVICE.replace("$this->missingTwo();", "$this->first();");
    let second = edit(&backend, URI, &after, Some(&baseline));
    assert_eq!(
        second.kind,
        SlowPassKind::Full(FullPassReason::ProjectChanged)
    );
    assert!(second.baseline.is_some());
}

#[test]
fn a_pass_over_text_the_index_has_not_caught_up_with_is_not_kept() {
    let backend = backend_with(&[(URI, SERVICE)]);
    let baseline = backend
        .collect_slow_diagnostics_incremental(URI, &Arc::new(SERVICE.to_string()), None)
        .baseline
        .unwrap();

    // The buffer moved on, but its parse has not been published yet.
    let after = Arc::new(SERVICE.replace("missingTwo", "missingTwX"));
    backend
        .open_files
        .write()
        .insert(URI.to_string(), Arc::clone(&after));
    let pass = backend.collect_slow_diagnostics_incremental(URI, &after, Some(&baseline));
    assert_eq!(pass.kind, SlowPassKind::Full(FullPassReason::IndexBehind));
    assert!(pass.baseline.is_none());
}

#[test]
fn a_file_that_was_not_open_when_parsed_is_never_kept() {
    let backend = Backend::new_test();
    backend.update_ast(URI, SERVICE);
    let content = Arc::new(SERVICE.to_string());
    backend
        .open_files
        .write()
        .insert(URI.to_string(), Arc::clone(&content));
    let pass = backend.collect_slow_diagnostics_incremental(URI, &content, None);
    assert_eq!(pass.kind, SlowPassKind::Full(FullPassReason::IndexBehind));
    assert!(pass.baseline.is_none());
}

#[tokio::test]
async fn a_save_makes_the_next_pass_diagnose_the_whole_file() {
    use tower_lsp_server::gen_lsp_types::{DidSaveTextDocumentParams, TextDocumentIdentifier, Uri};

    let backend = backend_with(&[(URI, SERVICE)]);
    let content = backend.open_files.read().get(URI).cloned().unwrap();
    backend.publish_diagnostics_for_file(URI, &content).await;
    assert!(backend.diag.slow_baselines.lock().contains_key(URI));

    backend
        .on_did_save(DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: Uri::parse(URI).unwrap(),
            },
            text: None,
        })
        .await;
    assert!(backend.diag.full_slow_pass_requested.lock().contains(URI));

    // The request is the next pass's to consume, whatever baseline a pass
    // still running at the time of the save leaves behind.
    backend.publish_diagnostics_for_file(URI, &content).await;
    assert!(!backend.diag.full_slow_pass_requested.lock().contains(URI));
    assert_eq!(
        canonical(backend.diag.last_slow.lock().get(URI).cloned().unwrap()),
        full_pass(&backend, URI, SERVICE)
    );
}

/// Every function and method body of the diagnostics demo, emptied and
/// prefixed in turn, re-diagnoses to what a pass over the whole file
/// reports.  The demo holds diagnostics on purpose, so this exercises both
/// moving the ones outside the edit and recomputing the ones inside it.
#[test]
fn partial_passes_match_full_passes_on_the_diagnostics_demo() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/php");
    let backend = Backend::new_test();
    let mut demo = None;
    for path in [
        "scaffolding/scaffolding.php",
        "completion.php",
        "definition.php",
        "diagnostics.php",
    ] {
        let content = std::fs::read_to_string(format!("{root}/{path}")).unwrap();
        let uri = format!("file:///demo/{path}");
        open(&backend, &uri, &content);
        if path == "diagnostics.php" {
            demo = Some((uri, content));
        }
    }
    let (uri, content) = demo.unwrap();
    let files = std::iter::once((uri, content));
    assert_eq!(verify_project(&backend, files, usize::MAX), 0);
}

/// Edits every function and method body of a real project and checks each
/// partial pass against a pass over the whole file.
///
/// Not part of the regular suite.  Point `PHPANTOM_INCREMENTAL_VERIFY_ROOT`
/// at a project and run with `--ignored --nocapture`; optionally narrow it
/// with `PHPANTOM_INCREMENTAL_VERIFY_FILTER` (a path substring) and cap the
/// bodies edited per file with `PHPANTOM_INCREMENTAL_VERIFY_UNITS`.
#[test]
#[ignore]
fn partial_passes_match_full_passes_on_a_project() {
    let Some(root) = std::env::var_os("PHPANTOM_INCREMENTAL_VERIFY_ROOT") else {
        return;
    };
    let root = std::fs::canonicalize(root).expect("project root");
    let filter = std::env::var("PHPANTOM_INCREMENTAL_VERIFY_FILTER").unwrap_or_default();
    let max_units: usize = std::env::var("PHPANTOM_INCREMENTAL_VERIFY_UNITS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(usize::MAX);

    // Opening a project parses it, which needs the stack the parser is
    // budgeted for.
    let mismatches = std::thread::Builder::new()
        .stack_size(crate::PARSE_WORKER_STACK_SIZE)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .thread_stack_size(crate::PARSE_WORKER_STACK_SIZE)
                .enable_all()
                .build()
                .unwrap();
            let cfg = crate::analyse::load_config_or_default(&root, None);
            let project = runtime
                .block_on(crate::analyse::open_project(&root, cfg, &[]))
                .expect("a project with PHP files");
            let backend = project.backend;
            let files = crate::analyse::parse_user_files(&backend, &root, &project.files, false);
            crate::analyse::discover_laravel_resources(&backend);
            verify_project(
                &backend,
                files
                    .into_iter()
                    .flatten()
                    .filter(|(uri, _)| uri.contains(&filter)),
                max_units,
            )
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(mismatches, 0, "partial passes disagreed with full passes");
}

fn verify_project(
    backend: &Backend,
    files: impl Iterator<Item = (String, String)>,
    max_units: usize,
) -> usize {
    use std::collections::BTreeMap;

    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatches = 0;
    let started = std::time::Instant::now();
    let mut partial_time = std::time::Duration::ZERO;
    let mut full_time = std::time::Duration::ZERO;

    for (uri, content) in files {
        let original = open(backend, &uri, &content);
        let first = backend.collect_slow_diagnostics_incremental(&uri, &original, None);
        if first.kind == SlowPassKind::Full(FullPassReason::Template) {
            *kinds.entry("template".to_string()).or_default() += 1;
            continue;
        }
        // A first pass may load classes and so not qualify as a baseline.
        let baseline = first.baseline.or_else(|| {
            (0..2).find_map(|_| {
                backend
                    .collect_slow_diagnostics_incremental(&uri, &original, None)
                    .baseline
            })
        });
        let Some(baseline) = baseline else {
            // In the editor such a file only ever gets full passes.
            eprintln!("NO BASELINE {uri}");
            *kinds.entry("no baseline".to_string()).or_default() += 1;
            continue;
        };

        let units = crate::parser::with_parsed_program(&content, "verify", |program, _| {
            super::units::collect(program)
        });
        for unit in units.iter().take(max_units) {
            let (open_brace, close_brace) = (unit.body.0 as usize, unit.body.1 as usize - 1);
            let edits = [
                (
                    "prepend",
                    format!(
                        "{}\n$phpantomProbe = 1;{}",
                        &content[..=open_brace],
                        &content[open_brace + 1..]
                    ),
                ),
                (
                    "empty",
                    format!("{}{}", &content[..=open_brace], &content[close_brace..]),
                ),
            ];
            for (label, edited) in edits {
                let edited = open(backend, &uri, &edited);
                let t0 = std::time::Instant::now();
                let pass =
                    backend.collect_slow_diagnostics_incremental(&uri, &edited, Some(&baseline));
                let incremental = t0.elapsed();
                *kinds.entry(format!("{:?}", pass.kind)).or_default() += 1;
                if matches!(pass.kind, SlowPassKind::Full(_)) {
                    continue;
                }
                partial_time += incremental;
                let t0 = std::time::Instant::now();
                let full = full_pass(backend, &uri, &edited);
                full_time += t0.elapsed();
                let partial = canonical(pass.diagnostics);
                if partial != full {
                    mismatches += 1;
                    let missing: Vec<_> = full.iter().filter(|d| !partial.contains(d)).collect();
                    let extra: Vec<_> = partial.iter().filter(|d| !full.contains(d)).collect();
                    eprintln!(
                        "MISMATCH {uri} {}() [{label}]\n  missing: {missing:#?}\n  extra: {extra:#?}",
                        unit.name(&content)
                    );
                }
            }
        }
        open(backend, &uri, &content);
        backend.open_files.write().remove(&uri);
        backend.diag.epoch.forget(&uri);
    }

    eprintln!(
        "{kinds:#?}\nmismatches: {mismatches}\nelapsed: {:?}; the partial passes took {:?}, full passes over the same edits {:?}",
        started.elapsed(),
        partial_time,
        full_time
    );
    mismatches
}

mod units_and_names {
    use super::super::units;
    use super::*;

    fn units_of(content: &str) -> Vec<units::DiagnosticUnit> {
        crate::parser::with_parsed_program(content, "test", |program, _| units::collect(program))
    }

    #[test]
    fn every_function_and_method_body_is_a_unit() {
        let content = r#"<?php
namespace App;

function top(): void {}

if (!function_exists('guarded')) {
    function guarded(): void {}
}

interface Contract { public function shape(): void; }

trait Helper { public function help(): void {} }

enum Kind { case A; public function label(): string { return ''; } }

abstract class Base
{
    abstract public function todo(): void;
    public function done(): void { $x = function () {}; }
}
"#;
        let names: Vec<&str> = units_of(content)
            .iter()
            .map(|unit| unit.name(content))
            .collect();
        assert_eq!(names, vec!["top", "guarded", "help", "label", "done"]);
    }

    #[test]
    fn only_a_change_confined_to_bodies_is_reported() {
        let old = SERVICE;
        let new = SERVICE.replace("missingTwo", "missingTwoAndMore");
        assert_eq!(
            units::changed_units(old, &units_of(old), &new, &units_of(&new)),
            Some(vec![1])
        );

        let renamed = SERVICE.replace("function third", "function fourth");
        assert_eq!(
            units::changed_units(old, &units_of(old), &renamed, &units_of(&renamed)),
            None
        );

        let added = SERVICE.replace(
            "class Service\n{\n",
            "class Service\n{\n    public function zeroth(): void {}\n",
        );
        assert_eq!(
            units::changed_units(old, &units_of(old), &added, &units_of(&added)),
            None
        );
    }
}
