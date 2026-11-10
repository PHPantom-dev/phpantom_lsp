//! Drupal's form and render callbacks: the methods and functions a callback
//! key of a form or render array names, as `'::method'`, `'Class::method'`,
//! a class name joined to `'::method'`, or a function name.

use crate::common::{apply_edits, edits_for_uri, hover_text_at, prepare_rename, rename};
use crate::drupal_services::{
    definitions_at, position_in, references_at, uri_for, workspace, workspace_in,
};
use tower_lsp_server::gen_lsp_types::*;

const FORM_INTERFACE_PATH: &str = "web/core/lib/Drupal/Core/Form/FormInterface.php";

const FORM_INTERFACE: &str = "<?php\nnamespace Drupal\\Core\\Form;\n\ninterface FormInterface {\n    public function buildForm(array $form, $form_state);\n}\n";

const FORM_BASE_PATH: &str = "web/core/lib/Drupal/Core/Form/FormBase.php";

const FORM_BASE: &str =
    "<?php\nnamespace Drupal\\Core\\Form;\n\nabstract class FormBase implements FormInterface {}\n";

/// A namespaced class and a global class that share the short name
/// `Helper`, and a namespaced function and a global function that share the
/// short name `acme_log_submit`.
const HELPER_PATH: &str = "web/modules/custom/acme/src/Helper.php";

const HELPER: &str = "<?php\nnamespace Drupal\\acme;\n\nclass Helper {\n    public function shout(): void {}\n    public function whisper(): void {}\n}\n";

const GLOBAL_HELPER_PATH: &str = "web/modules/custom/acme/global_helper.php";

const GLOBAL_HELPER: &str =
    "<?php\n\nclass Helper {\n    public static function shout(): void {}\n}\n";

const MODULE_PATH: &str = "web/modules/custom/acme/acme.module";

const MODULE: &str = "<?php\n\nfunction acme_log_submit(): void {}\n";

const FORM_FUNCTIONS_PATH: &str = "web/modules/custom/acme/src/Form/functions.php";

const FORM_FUNCTIONS: &str =
    "<?php\nnamespace Drupal\\acme\\Form;\n\nfunction acme_log_submit(): void {}\n";

const FORM_PATH: &str = "web/modules/custom/acme/src/Form/ExampleForm.php";

const FORM: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;
use Drupal\acme\Helper;

final class ExampleForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        $form['#validate'][] = '::checkTitle';
        $form['actions']['submit'] = [
            '#submit' => [
                '::saveTitle',
                'Helper::shout',
                '\Drupal\acme\Helper::whisper',
                Helper::class . '::shout',
                static::class . '::saveTitle',
                'acme_log_submit',
            ],
            '#ajax' => ['callback' => '::refresh'],
        ];
        $form['title']['#ajax']['callback'] = __CLASS__ . '::refresh';
        $form['#process'] = [self::class . '::checkTitle'];
        $form['#lazy_builder'] = ['Drupal\acme\Helper::whisper', []];
        $form['other'] = ['callback' => '::refresh'];
        $label = '::refresh';
        return $form;
    }

    public static function processTitle(array $element): array {
        $element['#process'][] = '::checkTitle';
        return $element;
    }

    public function checkTitle(): void {}

    public function saveTitle(): void {}

    public function refresh(): void {}
}
"#;

/// A caller in another namespace that imports neither `Helper`.
const BUILDER_PATH: &str = "web/modules/custom/acme/src/Render/Builder.php";

const BUILDER: &str = r#"<?php
namespace Drupal\acme\Render;

final class Builder {
    public function build(): array {
        return [
            '#pre_render' => ['Helper::shout', 'Drupal\acme\Helper::shout'],
            '#post_render' => ['acme_log_submit'],
        ];
    }
}
"#;

/// A hook class, which is no form: `'::validate'` names a method of a form
/// it does not know.
const HOOKS_PATH: &str = "web/modules/custom/acme/src/Hook/AcmeHooks.php";

const HOOKS: &str = r#"<?php
namespace Drupal\acme\Hook;

final class AcmeHooks {
    public function formAlter(array &$form): void {
        $form['#validate'][] = '::validate';
    }

    public function validate(): void {}
}
"#;

const TRAIT_PATH: &str = "web/modules/custom/acme/src/Form/ButtonsTrait.php";

const TRAIT: &str = r#"<?php
namespace Drupal\acme\Form;

trait ButtonsTrait {
    public function addButtons(array &$form): void {
        $form['#submit'][] = '::pressButton';
        $form['#validate'][] = static::class . '::pressButton';
    }

    public function pressButton(): void {}
}
"#;

/// The site with every fixture open.
async fn site() -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (HELPER_PATH, HELPER),
        (GLOBAL_HELPER_PATH, GLOBAL_HELPER),
        (MODULE_PATH, MODULE),
        (FORM_FUNCTIONS_PATH, FORM_FUNCTIONS),
        (FORM_PATH, FORM),
        (BUILDER_PATH, BUILDER),
        (HOOKS_PATH, HOOKS),
        (TRAIT_PATH, TRAIT),
    ])
    .await;
    backend.ensure_workspace_indexed();
    (backend, dir)
}

/// The zero-based line of the first occurrence of `needle` in `content`.
fn line_of(content: &str, needle: &str) -> u32 {
    content[..content.find(needle).expect("needle should exist")]
        .matches('\n')
        .count() as u32
}

/// The files and lines the definitions at `inside` bytes into the first
/// occurrence of `needle` in the file `path` point at.
async fn definition_lines(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    (path, content): (&str, &str),
    needle: &str,
    inside: usize,
) -> Vec<(Uri, u32)> {
    definitions_at(
        backend,
        &uri_for(dir, path),
        position_in(content, needle, inside),
    )
    .await
    .into_iter()
    .map(|location| (location.uri, location.range.start.line))
    .collect()
}

#[tokio::test]
async fn each_callback_shape_goes_to_what_it_names() {
    let (backend, dir) = site().await;
    let form = (FORM_PATH, FORM);
    let form_uri = uri_for(&dir, FORM_PATH);
    let helper = (uri_for(&dir, HELPER_PATH), line_of(HELPER, "shout"));
    let whisper = (uri_for(&dir, HELPER_PATH), line_of(HELPER, "whisper"));
    let global_shout = (
        uri_for(&dir, GLOBAL_HELPER_PATH),
        line_of(GLOBAL_HELPER, "shout"),
    );
    let global_function = (uri_for(&dir, MODULE_PATH), line_of(MODULE, "acme_log"));
    let check_title = (form_uri.clone(), line_of(FORM, "function checkTitle"));
    let save_title = (form_uri.clone(), line_of(FORM, "function saveTitle"));
    let refresh = (form_uri.clone(), line_of(FORM, "function refresh"));

    for (needle, inside, expected) in [
        ("'::checkTitle'", 4, &check_title),
        ("'::saveTitle'", 4, &save_title),
        ("'::refresh'", 4, &refresh),
        // A class name in a callback string is a runtime name: the import
        // of `Drupal\acme\Helper` does not apply to it.
        ("'Helper::shout'", 10, &global_shout),
        ("'\\Drupal\\acme\\Helper::whisper'", 23, &whisper),
        // A class name written in code does go through the imports.
        ("Helper::class . '::shout'", 20, &helper),
        ("static::class . '::saveTitle'", 21, &save_title),
        ("__CLASS__ . '::refresh'", 16, &refresh),
        ("self::class . '::checkTitle'", 19, &check_title),
        ("'Drupal\\acme\\Helper::whisper', []", 22, &whisper),
        // A function name in a callback string is a global name, never one
        // of the file's namespace.
        ("'acme_log_submit'", 3, &global_function),
        // `'::checkTitle'` in a static method of the form still names a
        // method of the form object.
        ("$element['#process'][] = '::checkTitle'", 30, &check_title),
    ] {
        let found = definition_lines(&backend, &dir, form, needle, inside).await;
        assert_eq!(found, vec![expected.clone()], "{needle}");
    }

    let builder = (BUILDER_PATH, BUILDER);
    for (needle, inside, expected) in [
        ("'Helper::shout'", 10, &global_shout),
        ("'Drupal\\acme\\Helper::shout'", 22, &helper),
        ("'acme_log_submit'", 3, &global_function),
    ] {
        let found = definition_lines(&backend, &dir, builder, needle, inside).await;
        assert_eq!(found, vec![expected.clone()], "{needle}");
    }

    let in_trait = (uri_for(&dir, TRAIT_PATH), line_of(TRAIT, "function press"));
    for (needle, inside) in [
        ("'::pressButton'", 4),
        ("static::class . '::pressButton'", 21),
    ] {
        let found = definition_lines(&backend, &dir, (TRAIT_PATH, TRAIT), needle, inside).await;
        assert_eq!(found, vec![in_trait.clone()], "{needle}");
    }
}

#[tokio::test]
async fn a_hover_on_a_callback_shows_what_it_names() {
    let (backend, dir) = site().await;
    let form_uri = uri_for(&dir, FORM_PATH);
    for (needle, inside, expected, unexpected) in [
        ("'::saveTitle'", 4, "saveTitle", None),
        // Only the global `Helper` declares a static `shout()`.
        ("'Helper::shout'", 10, "static function shout", None),
        ("static::class . '::saveTitle'", 21, "saveTitle", None),
        (
            "'acme_log_submit'",
            3,
            "acme_log_submit",
            Some("Drupal\\acme\\Form"),
        ),
    ] {
        let position = position_in(FORM, needle, inside);
        let hover = hover_text_at(&backend, &form_uri, position.line, position.character)
            .await
            .unwrap_or_else(|| panic!("no hover on {needle}"));
        assert!(hover.contains(expected), "{needle}: {hover}");
        if let Some(unexpected) = unexpected {
            assert!(!hover.contains(unexpected), "{needle}: {hover}");
        }
    }
}

/// The references of the name `inside` bytes into `needle` in the file
/// `path`, declarations included.
async fn reference_lines(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    (path, content): (&str, &str),
    needle: &str,
    inside: usize,
) -> Vec<(Uri, u32)> {
    references_at(
        backend,
        &uri_for(dir, path),
        position_in(content, needle, inside),
    )
    .await
}

/// A file's path and content.
type File = (&'static str, &'static str);

/// A place in a file: its path, its content, and the text found there.
type Place = (&'static str, &'static str, &'static str);

/// `(file, line)` for the first occurrence of each place's text in its
/// file, sorted the way [`references_at`] sorts them.
fn lines(dir: &tempfile::TempDir, places: &[Place]) -> Vec<(Uri, u32)> {
    let mut found: Vec<(Uri, u32)> = places
        .iter()
        .map(|(path, content, needle)| (uri_for(dir, path), line_of(content, needle)))
        .collect();
    found.sort();
    found
}

#[tokio::test]
async fn the_references_of_a_method_or_function_list_the_callbacks_naming_it() {
    let (backend, dir) = site().await;
    let form = (FORM_PATH, FORM);
    let cases: [(File, &str, usize, Vec<Place>); 7] = [
        (
            form,
            "function saveTitle",
            10,
            vec![
                (FORM_PATH, FORM, "function saveTitle"),
                (FORM_PATH, FORM, "'::saveTitle'"),
                (FORM_PATH, FORM, "static::class . '::saveTitle'"),
            ],
        ),
        (
            form,
            "function checkTitle",
            10,
            vec![
                (FORM_PATH, FORM, "function checkTitle"),
                (FORM_PATH, FORM, "$form['#validate'][] = '::checkTitle'"),
                (FORM_PATH, FORM, "self::class . '::checkTitle'"),
                (FORM_PATH, FORM, "$element['#process'][] = '::checkTitle'"),
            ],
        ),
        // Neither a bare `callback` key nor a string outside a callback key
        // names anything.
        (
            form,
            "function refresh",
            10,
            vec![
                (FORM_PATH, FORM, "function refresh"),
                (FORM_PATH, FORM, "'#ajax' => ['callback' => '::refresh']"),
                (FORM_PATH, FORM, "__CLASS__ . '::refresh'"),
            ],
        ),
        (
            (HELPER_PATH, HELPER),
            "function whisper",
            10,
            vec![
                (HELPER_PATH, HELPER, "function whisper"),
                (FORM_PATH, FORM, "'\\Drupal\\acme\\Helper::whisper'"),
                (FORM_PATH, FORM, "'Drupal\\acme\\Helper::whisper', []"),
            ],
        ),
        (
            (HELPER_PATH, HELPER),
            "function shout",
            10,
            vec![
                (HELPER_PATH, HELPER, "function shout"),
                (FORM_PATH, FORM, "Helper::class . '::shout'"),
                (BUILDER_PATH, BUILDER, "'Drupal\\acme\\Helper::shout'"),
            ],
        ),
        (
            (GLOBAL_HELPER_PATH, GLOBAL_HELPER),
            "function shout",
            10,
            vec![
                (GLOBAL_HELPER_PATH, GLOBAL_HELPER, "function shout"),
                (FORM_PATH, FORM, "'Helper::shout'"),
                (BUILDER_PATH, BUILDER, "'Helper::shout'"),
            ],
        ),
        (
            (MODULE_PATH, MODULE),
            "function acme_log_submit",
            10,
            vec![
                (MODULE_PATH, MODULE, "function acme_log_submit"),
                (FORM_PATH, FORM, "'acme_log_submit'"),
                (BUILDER_PATH, BUILDER, "'acme_log_submit'"),
            ],
        ),
    ];
    for (file, needle, inside, expected) in cases {
        let found = reference_lines(&backend, &dir, file, needle, inside).await;
        assert_eq!(found, lines(&dir, &expected), "{} {needle}", file.0);
    }

    for (file, needle, expected) in [
        (
            (FORM_FUNCTIONS_PATH, FORM_FUNCTIONS),
            "function acme_log_submit",
            (
                FORM_FUNCTIONS_PATH,
                FORM_FUNCTIONS,
                "function acme_log_submit",
            ),
        ),
        (
            (HOOKS_PATH, HOOKS),
            "function validate",
            (HOOKS_PATH, HOOKS, "function validate"),
        ),
    ] {
        let found = reference_lines(&backend, &dir, file, needle, 10).await;
        assert_eq!(found, lines(&dir, &[expected]), "{} {needle}", file.0);
    }

    let found = reference_lines(&backend, &dir, (TRAIT_PATH, TRAIT), "function press", 10).await;
    assert_eq!(
        found,
        lines(
            &dir,
            &[
                (TRAIT_PATH, TRAIT, "function press"),
                (TRAIT_PATH, TRAIT, "'::pressButton'"),
                (TRAIT_PATH, TRAIT, "static::class . '::pressButton'"),
            ]
        )
    );
}

#[tokio::test]
async fn a_callback_outside_a_form_or_a_callback_key_names_nothing() {
    let (backend, dir) = site().await;
    let found = definition_lines(&backend, &dir, (HOOKS_PATH, HOOKS), "'::validate'", 4).await;
    assert_eq!(found, Vec::new());
    let position = position_in(HOOKS, "'::validate'", 4);
    let hover = hover_text_at(
        &backend,
        &uri_for(&dir, HOOKS_PATH),
        position.line,
        position.character,
    )
    .await;
    assert_eq!(hover, None);
    for needle in [
        "$form['other'] = ['callback' => '::refresh']",
        "$label = '::refresh'",
    ] {
        let inside = needle.find("::refresh").unwrap() + 2;
        let found = definition_lines(&backend, &dir, (FORM_PATH, FORM), needle, inside).await;
        assert_eq!(found, Vec::new(), "{needle}");
    }
}

const HIGHLIGHT_PATH: &str = "web/modules/custom/acme/src/Form/highlighted.php";

/// A global function and a namespaced function that share a short name, and
/// callbacks that name the global one.
const HIGHLIGHT: &str = r#"<?php
namespace {
    function acme_highlight_submit(): void {}
}

namespace Drupal\acme\Form {
    function acme_highlight_submit(): void {}

    function build(array $form): array {
        $form['#submit'][] = 'acme_highlight_submit';
        $form['#validate'][] = '\acme_highlight_submit';
        acme_highlight_submit();
        return $form;
    }
}
"#;

#[tokio::test]
async fn a_highlight_links_a_function_and_the_callbacks_naming_it() {
    let (backend, dir) = workspace(&[(HIGHLIGHT_PATH, HIGHLIGHT)]).await;
    let uri = uri_for(&dir, HIGHLIGHT_PATH);
    let highlighted = |needle: &str, inside: usize| {
        let position = position_in(HIGHLIGHT, needle, inside);
        let mut lines: Vec<u32> = backend
            .handle_document_highlight(uri.as_str(), HIGHLIGHT, position)
            .unwrap_or_default()
            .iter()
            .map(|highlight| highlight.range.start.line)
            .collect();
        lines.sort();
        lines
    };
    let global = line_of(HIGHLIGHT, "function acme_highlight_submit");
    let namespaced = global + 4;
    let callbacks = [
        line_of(HIGHLIGHT, "'acme_highlight_submit'"),
        line_of(HIGHLIGHT, "'\\acme_highlight_submit'"),
    ];
    let call = line_of(HIGHLIGHT, "acme_highlight_submit();");
    for (needle, inside) in [
        ("'acme_highlight_submit'", 3),
        ("'\\acme_highlight_submit'", 3),
    ] {
        assert_eq!(
            highlighted(needle, inside),
            [global, callbacks[0], callbacks[1]],
            "{needle}"
        );
    }
    let found = highlighted("function acme_highlight_submit", 10);
    assert!(
        [global, callbacks[0], callbacks[1]]
            .iter()
            .all(|line| found.contains(line)),
        "{found:?}"
    );
    // The namespaced function and the call that names it are no callback's.
    for needle in [
        "acme_highlight_submit();",
        "    function acme_highlight_submit(): void {}\n\n    function build",
    ] {
        let inside = needle.find("acme").unwrap() + 2;
        let found = highlighted(needle, inside);
        assert!(found.contains(&call), "{needle}: {found:?}");
        assert!(found.contains(&namespaced), "{needle}: {found:?}");
        assert!(
            !callbacks.iter().any(|line| found.contains(line)),
            "{needle}: {found:?}"
        );
    }
}

const ESCAPED_PATH: &str = "web/modules/custom/acme/src/Form/escaped.php";

/// Callbacks that spell a namespaced function's name with escaped
/// backslashes, in a single-quoted and a double-quoted string.
const ESCAPED: &str = r#"<?php
namespace Drupal\acme\Form;

function acme_escaped_submit(): void {}

function escaped_build(array $form): array {
    $form['#submit'][] = 'Drupal\\acme\\Form\\acme_escaped_submit';
    $form['#validate'][] = "Drupal\\acme\\Form\\acme_escaped_submit";
    return $form;
}
"#;

#[tokio::test]
async fn renaming_a_function_rewrites_a_callback_that_escapes_its_backslashes() {
    let (backend, dir) = workspace(&[(ESCAPED_PATH, ESCAPED)]).await;
    backend.ensure_workspace_indexed();
    let uri = uri_for(&dir, ESCAPED_PATH);
    let expected = ESCAPED.replace("acme_escaped_submit", "acme_renamed_submit");
    for (needle, inside) in [
        ("function acme_escaped_submit", 10),
        ("'Drupal\\\\acme", 3),
        ("\"Drupal\\\\acme", 3),
    ] {
        let position = position_in(ESCAPED, needle, inside);
        assert!(
            prepare_rename(&backend, &uri, position.line, position.character)
                .await
                .is_some(),
            "{needle}"
        );
        let edit = rename(
            &backend,
            &uri,
            position.line,
            position.character,
            "acme_renamed_submit",
        )
        .await
        .unwrap_or_else(|| panic!("an edit from {needle}"));
        assert_eq!(
            renamed(&edit, &dir, (ESCAPED_PATH, ESCAPED)),
            expected,
            "{needle}"
        );
    }
}

/// The call hierarchy item of the callable at `inside` bytes into `needle`
/// in the file `path`.
async fn hierarchy_item(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    (path, content): (&str, &str),
    needle: &str,
    inside: usize,
) -> CallHierarchyItem {
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: uri_for(dir, path),
            },
            position: position_in(content, needle, inside),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    tower_lsp_server::LanguageServer::prepare_call_hierarchy(backend, params)
        .await
        .unwrap()
        .and_then(|items| items.into_iter().next())
        .unwrap_or_else(|| panic!("no callable at {needle}"))
}

#[tokio::test]
async fn the_call_hierarchy_counts_a_form_callback_as_a_call() {
    let (backend, dir) = site().await;
    let form = (FORM_PATH, FORM);
    let callback_lines = [
        line_of(FORM, "'::saveTitle'"),
        line_of(FORM, "static::class . '::saveTitle'"),
    ];

    let save_title = hierarchy_item(&backend, &dir, form, "function saveTitle", 10).await;
    let incoming = tower_lsp_server::LanguageServer::incoming_calls(
        &backend,
        CallHierarchyIncomingCallsParams {
            item: save_title,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    )
    .await
    .unwrap()
    .unwrap_or_default();
    let calls: Vec<(String, Vec<u32>)> = incoming
        .iter()
        .map(|call| {
            let mut lines: Vec<u32> = call.from_ranges.iter().map(|r| r.start.line).collect();
            lines.sort();
            (call.from.name.clone(), lines)
        })
        .collect();
    assert_eq!(calls, [("buildForm".to_string(), callback_lines.to_vec())]);

    let build_form = hierarchy_item(&backend, &dir, form, "function buildForm", 10).await;
    let outgoing = tower_lsp_server::LanguageServer::outgoing_calls(
        &backend,
        CallHierarchyOutgoingCallsParams {
            item: build_form,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    )
    .await
    .unwrap()
    .unwrap_or_default();
    let to_save_title = outgoing
        .iter()
        .find(|call| call.to.name == "saveTitle")
        .expect("a call to saveTitle");
    let mut lines: Vec<u32> = to_save_title
        .from_ranges
        .iter()
        .map(|r| r.start.line)
        .collect();
    lines.sort();
    assert_eq!(lines, callback_lines);

    // `'::validate'` outside a form names no method of it.
    let validate =
        hierarchy_item(&backend, &dir, (HOOKS_PATH, HOOKS), "function validate", 10).await;
    let incoming = tower_lsp_server::LanguageServer::incoming_calls(
        &backend,
        CallHierarchyIncomingCallsParams {
            item: validate,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        },
    )
    .await
    .unwrap()
    .unwrap_or_default();
    assert!(incoming.is_empty(), "{incoming:?}");
}

const INVOKED_PATH: &str = "web/modules/custom/acme/src/Form/invoked.php";

/// A callback that names an invokable class and one that names a function.
const INVOKED: &str = r#"<?php
namespace Drupal\acme\Form;

final class Invoked {
    public function __invoke(): void {}
}

function acme_invoked_submit(): void {}

function invoked_build(array $form): array {
    $form['#submit'][] = 'Drupal\acme\Form\Invoked';
    $form['#validate'][] = 'Drupal\acme\Form\acme_invoked_submit';
    return $form;
}
"#;

#[tokio::test]
async fn a_callback_naming_no_function_is_no_function_to_colour_or_rename() {
    let (backend, dir) = workspace(&[(INVOKED_PATH, INVOKED)]).await;
    let mut config = phpantom_lsp::config::Config::default();
    config.semantic_tokens.mode = Some(phpantom_lsp::config::SemanticTokensMode::Full);
    backend.set_config(config);
    let uri = uri_for(&dir, INVOKED_PATH);
    // The start of each token colouring a name as a function.
    let mut functions = Vec::new();
    let (mut line, mut character) = (0, 0);
    for token in backend
        .handle_semantic_tokens_full(uri.as_str(), INVOKED)
        .map(|tokens| tokens.data)
        .unwrap_or_default()
    {
        if token.delta_line > 0 {
            character = 0;
        }
        line += token.delta_line;
        character += token.delta_start;
        if token.token_type == 9 {
            functions.push(Position::new(line, character));
        }
    }
    for (needle, named) in [
        ("'Drupal\\acme\\Form\\Invoked'", false),
        ("'Drupal\\acme\\Form\\acme_invoked_submit'", true),
    ] {
        let position = position_in(INVOKED, needle, 1);
        assert_eq!(
            functions.contains(&position),
            named,
            "{needle}: {functions:?}"
        );
        let renameable = prepare_rename(&backend, &uri, position.line, position.character)
            .await
            .is_some();
        assert_eq!(renameable, named, "{needle}");
        let edit = rename(
            &backend,
            &uri,
            position.line,
            position.character,
            "acme_other",
        )
        .await;
        assert_eq!(edit.is_some(), named, "{needle}");
    }
}

/// The content of the file `path` once `edit` is applied to it.
fn renamed(edit: &WorkspaceEdit, dir: &tempfile::TempDir, (path, content): (&str, &str)) -> String {
    apply_edits(content, &edits_for_uri(edit, &uri_for(dir, path)))
}

#[tokio::test]
async fn renaming_a_method_or_function_rewrites_the_name_in_its_callbacks() {
    let (backend, dir) = site().await;

    let position = position_in(FORM, "function saveTitle", 10);
    let edit = rename(
        &backend,
        &uri_for(&dir, FORM_PATH),
        position.line,
        position.character,
        "storeTitle",
    )
    .await
    .expect("an edit");
    assert_eq!(
        renamed(&edit, &dir, (FORM_PATH, FORM)),
        FORM.replace("saveTitle", "storeTitle")
    );

    let position = position_in(HELPER, "function whisper", 10);
    let edit = rename(
        &backend,
        &uri_for(&dir, HELPER_PATH),
        position.line,
        position.character,
        "murmur",
    )
    .await
    .expect("an edit");
    assert_eq!(
        renamed(&edit, &dir, (FORM_PATH, FORM)),
        FORM.replace("whisper", "murmur")
    );

    let position = position_in(MODULE, "function acme_log_submit", 10);
    let edit = rename(
        &backend,
        &uri_for(&dir, MODULE_PATH),
        position.line,
        position.character,
        "acme_note_submit",
    )
    .await
    .expect("an edit");
    for file in [
        (FORM_PATH, FORM),
        (BUILDER_PATH, BUILDER),
        (MODULE_PATH, MODULE),
    ] {
        assert_eq!(
            renamed(&edit, &dir, file),
            file.1.replace("acme_log_submit", "acme_note_submit"),
            "{}",
            file.0
        );
    }
    assert!(edits_for_uri(&edit, &uri_for(&dir, FORM_FUNCTIONS_PATH)).is_empty());

    let position = position_in(HOOKS, "function validate", 10);
    let edit = rename(
        &backend,
        &uri_for(&dir, HOOKS_PATH),
        position.line,
        position.character,
        "verify",
    )
    .await
    .expect("an edit");
    assert_eq!(
        renamed(&edit, &dir, (HOOKS_PATH, HOOKS)),
        HOOKS.replace("function validate", "function verify")
    );
}

/// Callbacks that name nothing, or a method Drupal cannot call, which this
/// support reads without warning about, next to a call and a method call
/// that do get a warning.
const UNCHECKED_PATH: &str = "web/modules/custom/acme/src/Form/UncheckedForm.php";

const UNCHECKED: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;

final class UncheckedForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        $form['#submit'] = [
            '::nope',
            'Drupal\acme\Helper::nope',
            'Drupal\acme\Missing::nope',
            'acme_nope_submit',
            'Drupal\acme\Form\Guarded::hidden',
            'acme.greeter:greet',
        ];
        acme_missing_function();
        $this->missingMethod();
        return $form;
    }
}

final class Guarded {
    protected function hidden(): void {}
}
"#;

#[tokio::test]
async fn reading_callbacks_adds_no_warning() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (HELPER_PATH, HELPER),
        (UNCHECKED_PATH, UNCHECKED),
        (HOOKS_PATH, HOOKS),
    ])
    .await;
    backend.ensure_workspace_indexed();
    for (path, content, expected) in [
        (
            UNCHECKED_PATH,
            UNCHECKED,
            &["acme_missing_function()", "missingMethod"][..],
        ),
        (HOOKS_PATH, HOOKS, &[][..]),
    ] {
        let uri = uri_for(&dir, path);
        let mut diagnostics = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
        backend.collect_fast_diagnostics(uri.as_str(), content, &mut diagnostics);
        let messages: Vec<&str> = diagnostics
            .iter()
            .map(|diagnostic| crate::common::message_text(&diagnostic.message))
            .collect();
        assert_eq!(messages.len(), expected.len(), "{path}: {messages:?}");
        for name in expected {
            assert!(
                messages.iter().any(|message| message.contains(name)),
                "{path}: {messages:?}"
            );
        }
    }
}

/// The site with [`BUILDER`] on disk but not open, so the searches reach it
/// through the index rather than an open buffer.
async fn site_with_closed_builder() -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let full = dir.path().join(BUILDER_PATH);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, BUILDER).unwrap();
    let (backend, dir) = workspace_in(
        dir,
        &[
            (FORM_INTERFACE_PATH, FORM_INTERFACE),
            (FORM_BASE_PATH, FORM_BASE),
            (HELPER_PATH, HELPER),
            (GLOBAL_HELPER_PATH, GLOBAL_HELPER),
            (MODULE_PATH, MODULE),
            (FORM_FUNCTIONS_PATH, FORM_FUNCTIONS),
            (FORM_PATH, FORM),
        ],
    )
    .await;
    backend.ensure_workspace_indexed();
    (backend, dir)
}

#[tokio::test]
async fn references_and_rename_start_from_the_callback_string_too() {
    let (backend, dir) = site_with_closed_builder().await;
    let form = (FORM_PATH, FORM);

    let found = reference_lines(&backend, &dir, form, "'::saveTitle'", 4).await;
    assert_eq!(
        found,
        lines(
            &dir,
            &[
                (FORM_PATH, FORM, "function saveTitle"),
                (FORM_PATH, FORM, "'::saveTitle'"),
                (FORM_PATH, FORM, "static::class . '::saveTitle'"),
            ]
        )
    );
    let found = reference_lines(&backend, &dir, form, "'acme_log_submit'", 3).await;
    assert_eq!(
        found,
        lines(
            &dir,
            &[
                (MODULE_PATH, MODULE, "function acme_log_submit"),
                (FORM_PATH, FORM, "'acme_log_submit'"),
                (BUILDER_PATH, BUILDER, "'acme_log_submit'"),
            ]
        )
    );

    let form_uri = uri_for(&dir, FORM_PATH);
    let position = position_in(FORM, "'acme_log_submit'", 3);
    match prepare_rename(&backend, &form_uri, position.line, position.character).await {
        Some(PrepareRenameResult::PrepareRenamePlaceholder(PrepareRenamePlaceholder {
            placeholder,
            ..
        })) => {
            assert_eq!(placeholder, "acme_log_submit");
        }
        other => panic!("expected a placeholder, got {other:?}"),
    }
    let edit = rename(
        &backend,
        &form_uri,
        position.line,
        position.character,
        "acme_note_submit",
    )
    .await
    .expect("an edit");
    for file in [
        (FORM_PATH, FORM),
        (BUILDER_PATH, BUILDER),
        (MODULE_PATH, MODULE),
    ] {
        assert_eq!(
            renamed(&edit, &dir, file),
            file.1.replace("acme_log_submit", "acme_note_submit"),
            "{}",
            file.0
        );
    }
    assert!(edits_for_uri(&edit, &uri_for(&dir, FORM_FUNCTIONS_PATH)).is_empty());

    let position = position_in(FORM, "'Helper::shout'", 10);
    let edit = rename(
        &backend,
        &form_uri,
        position.line,
        position.character,
        "yell",
    )
    .await
    .expect("an edit");
    assert_eq!(
        renamed(&edit, &dir, (FORM_PATH, FORM)),
        FORM.replacen("'Helper::shout'", "'Helper::yell'", 1)
    );
    assert_eq!(
        renamed(&edit, &dir, (BUILDER_PATH, BUILDER)),
        BUILDER.replacen("'Helper::shout'", "'Helper::yell'", 1)
    );
    assert_eq!(
        renamed(&edit, &dir, (GLOBAL_HELPER_PATH, GLOBAL_HELPER)),
        GLOBAL_HELPER.replace("shout", "yell")
    );
    assert!(edits_for_uri(&edit, &uri_for(&dir, HELPER_PATH)).is_empty());
}
