//! Drupal's form and render callbacks: the methods and functions a callback
//! key of a form or render array names, as `'::method'`, `'Class::method'`,
//! a class name joined to `'::method'`, `'service:method'`, or a function
//! name.

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

/// The zero-based line, code and text of each diagnostic of the open file
/// `path`.
fn diagnostics_of(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    path: &str,
    content: &str,
) -> Vec<(u32, String, String)> {
    let uri = uri_for(dir, path);
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    backend.collect_fast_diagnostics(uri.as_str(), content, &mut diagnostics);
    let mut found: Vec<(u32, String, String)> = diagnostics
        .iter()
        .map(|diagnostic| {
            let code = match &diagnostic.code {
                Some(Code::String(code)) => code.clone(),
                _ => String::new(),
            };
            (
                diagnostic.range.start.line,
                code,
                crate::common::message_text(&diagnostic.message).to_string(),
            )
        })
        .collect();
    found.sort();
    found
}

/// Whether `found` holds exactly one diagnostic on the line of `needle` in
/// `content`, under `code`, whose text contains `text`.
#[track_caller]
fn assert_reported(
    found: &[(u32, String, String)],
    content: &str,
    needle: &str,
    code: &str,
    text: &str,
) {
    let line = line_of(content, needle);
    let on_line: Vec<_> = found.iter().filter(|(at, _, _)| *at == line).collect();
    assert!(
        on_line.len() == 1 && on_line[0].1 == code && on_line[0].2.contains(text),
        "{needle}: {found:?}"
    );
}

const WARNED_PATH: &str = "web/modules/custom/acme/src/Form/WarnedForm.php";

const WARNED: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;

final class WarnedForm extends FormBase {
    public function buildForm(array $form, $form_state, $callback = null, string $prefix = ''): array {
        $form['#validate'][] = '::missingCheck';
        $form['#submit'] = [[$this, 'missingSave'], [static::class, 'processTitle']];
        $form['#submit'][] = [static::class, 'save'];
        $form['#process'][] = static::class . '::save';
        $form['x']['#ajax']['callback'] = static::class . '::missingRefresh';
        $form['y'] = ['#ajax' => ['callback' => [$this, 'missingAjax']]];
        $form['#process'] = ['Drupal\acme\Helper::missingProcess', '\Drupal\acme\Helper::shout'];
        $form['#markup'] = [$this, 'notACallback'];
        $form['z']['#ajax']['callback'] = '::save';
        $form['#after_build'][] = '::save';
        $form['#entity_builders'] = ['::save'];
        $form['#submit'][] = $callback;
        $form['#submit'][] = $prefix . '::notRead';
        return $form;
    }

    public static function processTitle(array $element): array {
        $element['#process'][] = '::missingInStatic';
        return $element;
    }

    public function save(): void {}
}
"#;

#[tokio::test]
async fn a_callback_naming_a_missing_method_is_reported() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (HELPER_PATH, HELPER),
        (WARNED_PATH, WARNED),
        (HOOKS_PATH, HOOKS),
        (TRAIT_PATH, MISSING_TRAIT),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, WARNED_PATH, WARNED);
    for needle in [
        "missingCheck",
        "missingSave",
        "missingRefresh",
        "missingAjax",
        "missingProcess",
        "missingInStatic",
    ] {
        assert_reported(&found, WARNED, needle, "unknown_member", needle);
    }
    // Drupal calls an array callable that names its class only when the
    // method is static, and instantiates the class a string names.
    assert_reported(
        &found,
        WARNED,
        "[static::class, 'save']",
        "invalid_drupal_callback",
        "WarnedForm::save() is not static",
    );
    assert_eq!(found.len(), 7, "{found:?}");

    // `'::method'` outside a form names a method of a form the code does
    // not know, and in a trait one the using form may declare.
    assert_eq!(
        diagnostics_of(&backend, &dir, HOOKS_PATH, HOOKS),
        Vec::new()
    );
    assert_eq!(
        diagnostics_of(&backend, &dir, TRAIT_PATH, MISSING_TRAIT),
        Vec::new()
    );
}

const KEYED_PATH: &str = "web/modules/custom/acme/src/Form/KeyedForm.php";

/// A form whose callback lists hold keyed entries, as core's entity forms
/// key their entity builders.
const KEYED: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;

final class KeyedForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        $form['#entity_builders']['acme_entity'] = '::buildEntity';
        $form['actions']['submit'] = ['#submit' => ['acme_save' => '::saveKeyed']];
        $form['#validate']['acme_missing'] = '::missingKeyed';
        $form['#ajax']['wrapper'] = '::buildEntity';
        $form['other']['acme_other'] = '::buildEntity';
        $form['#lazy_builder'] = ['acme_builder' => '::buildEntity', []];
        return $form;
    }

    public function buildEntity(): void {}

    public function saveKeyed(): void {}
}
"#;

#[tokio::test]
async fn a_keyed_entry_of_a_callback_list_is_a_callback() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (KEYED_PATH, KEYED),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let keyed = (KEYED_PATH, KEYED);
    let keyed_uri = uri_for(&dir, KEYED_PATH);
    let build_entity = (keyed_uri.clone(), line_of(KEYED, "function buildEntity"));
    let save_keyed = (keyed_uri.clone(), line_of(KEYED, "function saveKeyed"));
    for (needle, inside, expected) in [
        ("'::buildEntity'", 4, vec![build_entity]),
        ("'::saveKeyed'", 4, vec![save_keyed]),
        // Only a list key's entries are callbacks, and only the first item
        // of `#lazy_builder`.
        ("['wrapper'] = '::buildEntity'", 18, Vec::new()),
        ("['acme_other'] = '::buildEntity'", 21, Vec::new()),
        ("'acme_builder' => '::buildEntity'", 23, Vec::new()),
    ] {
        let found = definition_lines(&backend, &dir, keyed, needle, inside).await;
        assert_eq!(found, expected, "{needle}");
    }

    let found = reference_lines(&backend, &dir, keyed, "function buildEntity", 10).await;
    assert_eq!(
        found,
        lines(
            &dir,
            &[
                (KEYED_PATH, KEYED, "function buildEntity"),
                (KEYED_PATH, KEYED, "$form['#entity_builders']"),
            ]
        )
    );

    let position = position_in(KEYED, "function saveKeyed", 10);
    let edit = rename(
        &backend,
        &keyed_uri,
        position.line,
        position.character,
        "storeKeyed",
    )
    .await
    .expect("an edit");
    assert_eq!(
        renamed(&edit, &dir, keyed),
        KEYED.replace("saveKeyed", "storeKeyed")
    );

    let found = diagnostics_of(&backend, &dir, KEYED_PATH, KEYED);
    assert_reported(
        &found,
        KEYED,
        "missingKeyed",
        "unknown_member",
        "missingKeyed",
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

/// A trait whose callbacks name a method it lacks.
const MISSING_TRAIT: &str = r#"<?php
namespace Drupal\acme\Form;

trait ButtonsTrait {
    public function addButtons(array &$form): void {
        $form['#submit'][] = '::pressMissing';
        $form['#validate'][] = static::class . '::pressMissing';
        $form['#process'][] = [$this, 'pressMissing'];
    }
}
"#;

const HIDDEN_PATH: &str = "web/modules/custom/acme/src/Form/HiddenForm.php";

/// A form whose callbacks name methods Drupal cannot call from outside the
/// class, which the class itself may call.
const HIDDEN: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;

final class HiddenForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        $form['#validate'][] = '::privateCheck';
        $form['#submit'] = [[$this, 'protectedSave']];
        $form['#process'][] = static::class . '::privateProcess';
        $form['#pre_render'] = ['Drupal\acme\Form\Hidden::secret'];
        $this->privateCheck();
        $this->protectedSave();
        static::privateProcess();
        return $form;
    }

    private function privateCheck(): void {}

    protected function protectedSave(): void {}

    private static function privateProcess(): void {}
}

final class Hidden {
    protected function secret(): void {}
}
"#;

#[tokio::test]
async fn a_callback_naming_a_method_drupal_cannot_call_is_reported() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (HIDDEN_PATH, HIDDEN),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, HIDDEN_PATH, HIDDEN);
    for (needle, text) in [
        ("'::privateCheck'", "Cannot call private method"),
        ("'protectedSave'", "Cannot call protected method"),
        ("'::privateProcess'", "Cannot call private method"),
        ("Hidden::secret", "Cannot call protected method"),
    ] {
        assert_reported(&found, HIDDEN, needle, "inaccessible_member", text);
    }
    assert_eq!(found.len(), 4, "{found:?}");
}

const NAMESPACED_FUNCTION_PATH: &str = "web/modules/custom/acme/src/Form/namespaced.php";

/// A function only the form's namespace declares.
const NAMESPACED_FUNCTION: &str =
    "<?php\nnamespace Drupal\\acme\\Form;\n\nfunction acme_namespaced_submit(): void {}\n";

const INVOKABLE_PATH: &str = "web/modules/custom/acme/src/Invokable.php";

const INVOKABLE: &str = "<?php\nnamespace Drupal\\acme;\n\nfinal class Invokable {\n    public function __invoke(): void {}\n}\n";

const INVOKABLE_SERVICES_PATH: &str = "web/modules/custom/acme/acme.services.yml";

const INVOKABLE_SERVICES: &str =
    "services:\n  acme_invokable:\n    class: Drupal\\acme\\Invokable\n";

const FUNCTIONS_FORM_PATH: &str = "web/modules/custom/acme/src/Form/FunctionsForm.php";

const FUNCTIONS_FORM: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;

final class FunctionsForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        $form['#submit'] = [
            'acme_log_submit',
            'acme_missing_submit',
            'acme_namespaced_submit',
            'Drupal\acme\Form\acme_namespaced_submit',
            'Drupal\acme\Invokable',
            'acme_invokable',
            'acme_same_file_submit',
            'Drupal\acme\Form\acme_same_file_submit',
        ];
        $form['#markup'] = 'acme_missing_markup';
        return $form;
    }
}

function acme_same_file_submit(): void {}
"#;

#[tokio::test]
async fn a_callback_naming_a_missing_function_is_reported() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (MODULE_PATH, MODULE),
        (NAMESPACED_FUNCTION_PATH, NAMESPACED_FUNCTION),
        (INVOKABLE_PATH, INVOKABLE),
        (INVOKABLE_SERVICES_PATH, INVOKABLE_SERVICES),
        (FUNCTIONS_FORM_PATH, FUNCTIONS_FORM),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, FUNCTIONS_FORM_PATH, FUNCTIONS_FORM);
    // A function name is global: the form's namespace does not apply.  A
    // name that is a class or a service names an invokable.
    for needle in [
        "'acme_missing_submit'",
        "'acme_namespaced_submit'",
        "'acme_same_file_submit'",
    ] {
        assert_reported(
            &found,
            FUNCTIONS_FORM,
            needle,
            "unknown_function",
            "not found",
        );
    }
    assert_eq!(found.len(), 3, "{found:?}");
}

const GUARDED_PATH: &str = "web/modules/custom/acme/src/Form/GuardedForm.php";

/// A form that adds a callback only when its function exists.
const GUARDED: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;

final class GuardedForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        if (function_exists('acme_optional_submit')) {
            $form['#submit'][] = 'acme_optional_submit';
        }
        if (function_exists('\acme_other_submit')) {
            $form['#submit'][] = 'acme_other_submit';
        }
        $form['#validate'][] = 'acme_optional_submit';
        return $form;
    }
}
"#;

#[tokio::test]
async fn a_callback_function_guarded_by_function_exists_is_not_reported() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (INVOKABLE_SERVICES_PATH, INVOKABLE_SERVICES),
        (GUARDED_PATH, GUARDED),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, GUARDED_PATH, GUARDED);
    assert_reported(
        &found,
        GUARDED,
        "$form['#validate'][] = 'acme_optional_submit'",
        "unknown_function",
        "Function acme_optional_submit() not found",
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

const DEPRECATED_FORM_PATH: &str = "web/modules/custom/acme/src/Form/DeprecatedForm.php";

/// A form whose render callbacks name its methods as `'::method'`, which
/// Drupal resolves only in a form callback.
const DEPRECATED_FORM: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;

final class DeprecatedForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        $form['#pre_render'][] = '::oldRender';
        $form['#submit'][] = '::oldRender';
        $form['#post_render'][] = '::missingRender';
        $form['#value_callback'] = '::oldRender';
        return $form;
    }

    /** @deprecated Use buildForm() instead. */
    public function oldRender(): void {}
}
"#;

#[tokio::test]
async fn a_form_method_drupal_does_not_look_up_is_reported_only_as_such() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (DEPRECATED_FORM_PATH, DEPRECATED_FORM),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, DEPRECATED_FORM_PATH, DEPRECATED_FORM);
    for (needle, code, text) in [
        (
            "#pre_render",
            "invalid_drupal_callback",
            "only in a form callback",
        ),
        ("#submit", "deprecated_usage", "oldRender"),
        (
            "#post_render",
            "invalid_drupal_callback",
            "only in a form callback",
        ),
        ("#value_callback", "invalid_drupal_callback", "ignored"),
    ] {
        assert_reported(&found, DEPRECATED_FORM, needle, code, text);
    }
    assert_eq!(found.len(), 4, "{found:?}");
}

const MAGIC_PATH: &str = "web/modules/custom/acme/src/Magic.php";

/// A class whose instances answer any method, which its class does not.
const MAGIC: &str = "<?php\nnamespace Drupal\\acme;\n\nfinal class Magic {\n    public function __call(string $name, array $arguments): mixed { return null; }\n}\n";

const MAGIC_FORM_PATH: &str = "web/modules/custom/acme/src/Form/MagicForm.php";

const MAGIC_FORM: &str = r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;
use Drupal\acme\Magic;

final class MagicForm extends FormBase {
    public function buildForm(array $form, $form_state): array {
        $form['#submit'][] = 'Drupal\acme\Magic::anything';
        $form['#validate'][] = Magic::class . '::anything';
        $form['#process'][] = [Magic::class, 'anything'];
        return $form;
    }
}
"#;

#[tokio::test]
async fn a_class_callback_calls_the_method_of_an_instance() {
    let (backend, dir) = workspace(&[
        (FORM_INTERFACE_PATH, FORM_INTERFACE),
        (FORM_BASE_PATH, FORM_BASE),
        (MAGIC_PATH, MAGIC),
        (MAGIC_FORM_PATH, MAGIC_FORM),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, MAGIC_FORM_PATH, MAGIC_FORM);
    // Drupal instantiates the class a callback string names, but calls an
    // array callable that names its class statically.
    assert_reported(
        &found,
        MAGIC_FORM,
        "[Magic::class, 'anything']",
        "unknown_member",
        "anything",
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

const ELEMENT_PATH: &str = "web/modules/custom/acme/src/Element/Widget.php";

/// A form element whose render callbacks name `'::method'`, which Drupal
/// resolves only in form callbacks, and whose value callbacks Drupal
/// ignores unless they are native callables.
const ELEMENT: &str = r#"<?php
namespace Drupal\acme\Element;

final class Widget {
    public function info(): array {
        $info = [
            '#pre_render' => [
                '::preRender',
                static::class . '::preRender',
                'Drupal\acme\Missing::build',
                'acme_invokable::__invoke',
            ],
            '#lazy_builder' => ['::build', []],
            '#access_callback' => '::access',
        ];
        $info['#post_render'][] = '::postRender';
        $info['#value_callback'] = '::staticValue';
        $info['a']['#value_callback'] = 'acme_invokable:__invoke';
        $info['b']['#value_callback'] = 'Drupal\acme\Element\Widget::value';
        $info['c']['#value_callback'] = static::class . '::value';
        $info['d']['#value_callback'] = [static::class, 'value'];
        $info['e']['#value_callback'] = [static::class, 'staticValue'];
        $info['f']['#value_callback'] = 'Drupal\acme\Element\Widget::staticValue';
        $info['g']['#value_callback'] = [$this, 'value'];
        $info['h']['#value_callback'] = 'acme_log_submit';
        return $info;
    }

    public function preRender(): void {}

    public function postRender(): void {}

    public function build(): void {}

    public function access(): void {}

    public function value(): void {}

    public static function staticValue(): void {}
}
"#;

#[tokio::test]
async fn a_callback_drupal_cannot_resolve_or_ignores_is_reported() {
    let (backend, dir) = workspace(&[
        (MODULE_PATH, MODULE),
        (INVOKABLE_PATH, INVOKABLE),
        (INVOKABLE_SERVICES_PATH, INVOKABLE_SERVICES),
        (ELEMENT_PATH, ELEMENT),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, ELEMENT_PATH, ELEMENT);
    for needle in ["'::preRender'", "'::build'", "'::access'", "'::postRender'"] {
        assert_reported(
            &found,
            ELEMENT,
            needle,
            "invalid_drupal_callback",
            "only in a form callback",
        );
    }
    for needle in [
        "'::staticValue'",
        "'acme_invokable:__invoke'",
        "'Drupal\\acme\\Element\\Widget::value'",
        "static::class . '::value'",
        "[static::class, 'value']",
    ] {
        assert_reported(
            &found,
            ELEMENT,
            needle,
            "invalid_drupal_callback",
            "ignored",
        );
    }
    // The class part of `'Class::method'` names a class, or a service.
    assert_reported(
        &found,
        ELEMENT,
        "Missing::build",
        "unknown_class",
        "Class Drupal\\acme\\Missing not found",
    );
    assert_eq!(found.len(), 10, "{found:?}");
}

const IMPORTING_PATH: &str = "web/modules/custom/acme/src/Render/Importing.php";

/// A caller in another namespace that imports the namespaced `Helper`:
/// the class name in a callback string is read as fully qualified all the
/// same.
const IMPORTING: &str = r#"<?php
namespace Drupal\acme\Render;

use Drupal\acme\Helper;

final class Importing {
    public function build(): array {
        return [
            '#pre_render' => [
                'Helper::shout',
                'Helper::whisper',
                'Drupal\acme\Helper::whisper',
                Helper::class . '::whisper',
            ],
        ];
    }
}
"#;

#[tokio::test]
async fn the_class_of_a_callback_string_is_read_as_fully_qualified() {
    // Without a global `Helper`, the short name names no class.
    let (backend, dir) = workspace(&[
        (HELPER_PATH, HELPER),
        (INVOKABLE_SERVICES_PATH, INVOKABLE_SERVICES),
        (IMPORTING_PATH, IMPORTING),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, IMPORTING_PATH, IMPORTING);
    for needle in ["'Helper::shout'", "'Helper::whisper'"] {
        assert_reported(
            &found,
            IMPORTING,
            needle,
            "unknown_class",
            "Class Helper not found",
        );
    }
    assert_eq!(found.len(), 2, "{found:?}");

    // With one, the short name names it, whatever the file imports.
    let (backend, dir) = workspace(&[
        (HELPER_PATH, HELPER),
        (GLOBAL_HELPER_PATH, GLOBAL_HELPER),
        (INVOKABLE_SERVICES_PATH, INVOKABLE_SERVICES),
        (IMPORTING_PATH, IMPORTING),
    ])
    .await;
    backend.ensure_workspace_indexed();
    let found = diagnostics_of(&backend, &dir, IMPORTING_PATH, IMPORTING);
    assert_reported(
        &found,
        IMPORTING,
        "'Helper::whisper'",
        "unknown_member",
        "Method Helper::whisper() not found",
    );
    assert_eq!(found.len(), 1, "{found:?}");
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

const BUILDERS_PATH: &str = "web/modules/custom/acme/src/GreetingBuilders.php";

/// Two service classes that both declare `build()` and `preRender()`.
const BUILDERS: &str = r#"<?php
namespace Drupal\acme;

final class GreetingBuilder {
    public function build(): array { return []; }
    public function preRender(array $element): array { return $element; }
}

final class FarewellBuilder {
    public function build(): array { return []; }
    public function preRender(array $element): array { return $element; }
}
"#;

const BUILDER_SERVICES_PATH: &str = "web/modules/custom/acme/acme.services.yml";

const BUILDER_SERVICES: &str = "services:\n  acme.greeting_builder:\n    class: Drupal\\acme\\GreetingBuilder\n  acme.greeting_alias:\n    alias: acme.greeting_builder\n  acme.greeting_shorthand: '@acme.greeting_builder'\n  acme.farewell_builder:\n    class: Drupal\\acme\\FarewellBuilder\n";

const CARD_PATH: &str = "web/modules/custom/acme/src/Render/Card.php";

const CARD: &str = r#"<?php
namespace Drupal\acme\Render;

final class Card {
    public function view(): array {
        $element = [
            '#lazy_builder' => ['acme.greeting_builder:build', []],
            '#pre_render' => ['acme.greeting_alias:preRender', 'acme.greeting_shorthand:preRender'],
        ];
        $element['#pre_render'][] = 'acme.greeting_builder:preRender';
        $element['#markup'] = 'acme.farewell_builder:preRender';
        return $element;
    }
}
"#;

/// A site whose services file declares a service, an alias of it in each
/// spelling, and a second service, with [`CARD`] closed when `card_open`
/// is false.
async fn service_site(card_open: bool) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut files = vec![
        (BUILDERS_PATH, BUILDERS),
        (BUILDER_SERVICES_PATH, BUILDER_SERVICES),
    ];
    if card_open {
        files.push((CARD_PATH, CARD));
    } else {
        let full = dir.path().join(CARD_PATH);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, CARD).unwrap();
    }
    let (backend, dir) = workspace_in(dir, &files).await;
    backend.ensure_workspace_indexed();
    (backend, dir)
}

#[tokio::test]
async fn a_service_callback_goes_to_the_method_of_the_service_class() {
    let (backend, dir) = service_site(true).await;
    let card = (CARD_PATH, CARD);
    let builders_uri = uri_for(&dir, BUILDERS_PATH);
    let build = (builders_uri.clone(), line_of(BUILDERS, "build()"));
    let pre_render = (builders_uri.clone(), line_of(BUILDERS, "preRender"));
    for (needle, inside, expected) in [
        ("'acme.greeting_builder:build'", 23, &build),
        // An alias, written either way, leads to its target's class.
        ("'acme.greeting_alias:preRender'", 22, &pre_render),
        ("'acme.greeting_shorthand:preRender'", 26, &pre_render),
        ("'acme.greeting_builder:preRender'", 24, &pre_render),
    ] {
        let found = definition_lines(&backend, &dir, card, needle, inside).await;
        assert_eq!(found, vec![expected.clone()], "{needle}");
    }

    let services_uri = uri_for(&dir, BUILDER_SERVICES_PATH);
    for (needle, declared) in [
        ("'acme.greeting_builder:build'", "  acme.greeting_builder:"),
        ("'acme.greeting_alias:preRender'", "  acme.greeting_alias:"),
    ] {
        let found = definition_lines(&backend, &dir, card, needle, 3).await;
        assert_eq!(
            found,
            vec![(services_uri.clone(), line_of(BUILDER_SERVICES, declared))],
            "{needle}"
        );
    }

    // A string outside a callback key names nothing.
    let found = definition_lines(&backend, &dir, card, "'acme.farewell_builder:", 24).await;
    assert_eq!(found, Vec::new());
}

#[tokio::test]
async fn a_hover_on_a_service_callback_shows_the_method_and_the_service() {
    let (backend, dir) = service_site(true).await;
    let card_uri = uri_for(&dir, CARD_PATH);
    for (needle, inside, expected) in [
        ("'acme.greeting_alias:preRender'", 22, "GreetingBuilder"),
        (
            "'acme.greeting_builder:build'",
            3,
            "**Service** `acme.greeting_builder`",
        ),
    ] {
        let position = position_in(CARD, needle, inside);
        let hover = hover_text_at(&backend, &card_uri, position.line, position.character)
            .await
            .unwrap_or_else(|| panic!("no hover on {needle}"));
        assert!(hover.contains(expected), "{needle}: {hover}");
    }
}

#[tokio::test]
async fn the_references_of_a_service_method_list_its_callbacks() {
    for card_open in [true, false] {
        let (backend, dir) = service_site(card_open).await;
        let expected = lines(
            &dir,
            &[
                (BUILDERS_PATH, BUILDERS, "public function preRender"),
                (CARD_PATH, CARD, "'acme.greeting_alias:preRender'"),
                (CARD_PATH, CARD, "'acme.greeting_shorthand:preRender'"),
                (CARD_PATH, CARD, "$element['#pre_render'][]"),
            ],
        );
        let found =
            reference_lines(&backend, &dir, (BUILDERS_PATH, BUILDERS), "preRender", 2).await;
        assert_eq!(found, expected, "card open: {card_open}");
        if card_open {
            let found = reference_lines(
                &backend,
                &dir,
                (CARD_PATH, CARD),
                "'acme.greeting_builder:preRender'",
                24,
            )
            .await;
            assert_eq!(found, expected);
        }

        // The other service's `preRender()` has no callback naming it.
        let farewell = BUILDERS.find("FarewellBuilder").unwrap();
        let farewell = farewell + BUILDERS[farewell..].find("preRender").unwrap();
        let found = references_at(
            &backend,
            &uri_for(&dir, BUILDERS_PATH),
            crate::common::offset_position(BUILDERS, farewell + 2),
        )
        .await;
        assert_eq!(
            found,
            vec![(
                uri_for(&dir, BUILDERS_PATH),
                BUILDERS[..farewell].matches('\n').count() as u32
            )],
            "card open: {card_open}"
        );
    }
}

#[tokio::test]
async fn renaming_a_service_method_rewrites_its_callbacks() {
    let (backend, dir) = service_site(false).await;
    let position = position_in(BUILDERS, "preRender", 2);
    let edit = rename(
        &backend,
        &uri_for(&dir, BUILDERS_PATH),
        position.line,
        position.character,
        "decorate",
    )
    .await
    .expect("an edit");
    assert_eq!(
        renamed(&edit, &dir, (CARD_PATH, CARD)),
        CARD.replace("greeting_alias:preRender", "greeting_alias:decorate")
            .replace(
                "greeting_shorthand:preRender",
                "greeting_shorthand:decorate"
            )
            .replace("greeting_builder:preRender", "greeting_builder:decorate")
    );
    assert_eq!(
        renamed(&edit, &dir, (BUILDERS_PATH, BUILDERS)),
        BUILDERS.replacen("preRender", "decorate", 1)
    );
}

#[tokio::test]
async fn the_references_of_a_service_list_its_callbacks() {
    let (backend, dir) = service_site(true).await;
    let found = references_at(
        &backend,
        &uri_for(&dir, BUILDER_SERVICES_PATH),
        position_in(BUILDER_SERVICES, "acme.greeting_builder:", 3),
    )
    .await;
    let card_uri = uri_for(&dir, CARD_PATH);
    for needle in ["'acme.greeting_builder:build'", "$element['#pre_render'][]"] {
        assert!(
            found.contains(&(card_uri.clone(), line_of(CARD, needle))),
            "{needle}: {found:?}"
        );
    }
    assert!(
        !found.contains(&(card_uri.clone(), line_of(CARD, "'#markup'"))),
        "{found:?}"
    );
}

const UNKNOWN_CARD_PATH: &str = "web/modules/custom/acme/src/Render/UnknownCard.php";

const UNKNOWN_CARD: &str = r#"<?php
namespace Drupal\acme\Render;

final class UnknownCard {
    public function view(): array {
        return [
            '#pre_render' => ['acme.missing_builder:build', 'acme.greeting_builder:nope'],
        ];
    }
}
"#;

#[tokio::test]
async fn a_service_callback_checks_its_service_like_any_service_id() {
    let (backend, dir) = workspace_in(
        tempfile::tempdir().unwrap(),
        &[
            (BUILDERS_PATH, BUILDERS),
            (BUILDER_SERVICES_PATH, BUILDER_SERVICES),
            (UNKNOWN_CARD_PATH, UNKNOWN_CARD),
        ],
    )
    .await;
    backend.ensure_workspace_indexed();
    let uri = uri_for(&dir, UNKNOWN_CARD_PATH);
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), UNKNOWN_CARD, &mut diagnostics);
    backend.collect_fast_diagnostics(uri.as_str(), UNKNOWN_CARD, &mut diagnostics);
    let messages: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| crate::common::message_text(&diagnostic.message))
        .collect();
    // An unknown service is reported as a service, and the method a
    // declared service's class lacks as a method.
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .any(|m| m.contains("'acme.missing_builder'")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("GreetingBuilder::nope()")),
        "{messages:?}"
    );
}

// ─── Completion ─────────────────────────────────────────────────────────────

/// Where a completion test puts the cursor in a buffer.
const CURSOR: &str = "<|>";

/// What accepting each item offered at [`CURSOR`] in `buffer`, opened as
/// the file `path`, writes, and where the text it replaces starts, as the
/// byte offset into `buffer` without the marker.
async fn completed(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    path: &str,
    buffer: &str,
) -> Vec<(String, usize)> {
    let at = buffer
        .find(CURSOR)
        .expect("the buffer should mark the cursor");
    let content = buffer.replacen(CURSOR, "", 1);
    let uri = uri_for(dir, path);
    crate::common::open_document(backend, &uri, "php", &content).await;
    let position = crate::common::offset_position(&content, at);
    let items = crate::common::response_items(
        crate::common::completion_response(backend, &uri, position.line, position.character, None)
            .await,
    )
    .unwrap_or_default();
    let mut written: Vec<(String, usize)> = items
        .into_iter()
        .map(|item| match item.text_edit {
            Some(CompletionItemTextEdit::TextEdit(edit)) => {
                assert_eq!(edit.range.end, position, "{}", item.label);
                let line_start: usize = content
                    .split_inclusive('\n')
                    .take(edit.range.start.line as usize)
                    .map(str::len)
                    .sum();
                (
                    edit.new_text,
                    line_start + edit.range.start.character as usize,
                )
            }
            _ => (item.insert_text.unwrap_or(item.label), usize::MAX),
        })
        .collect();
    written.sort();
    written
}

/// The names [`completed`] writes, each replacing the text from the
/// cursor's `typed` bytes back.
async fn completed_names(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    path: &str,
    buffer: &str,
    typed: usize,
) -> Vec<String> {
    let start = buffer.find(CURSOR).unwrap() - typed;
    completed(backend, dir, path, buffer)
        .await
        .into_iter()
        .map(|(name, at)| {
            assert_eq!(at, start, "{name} should replace only what was typed");
            name
        })
        .collect()
}

const EDITED_FORM_PATH: &str = "web/modules/custom/acme/src/Form/EditedForm.php";

/// A form being edited, its methods declared above the method that builds
/// it so that an unclosed string leaves them in place.
fn edited_form(line: &str) -> String {
    format!(
        r#"<?php
namespace Drupal\acme\Form;

use Drupal\Core\Form\FormBase;
use Drupal\acme\Helper;

final class EditedForm extends FormBase {{
    public function saveTitle(): void {{}}
    public static function processTitle(array $element): array {{ return $element; }}
    protected function helper(): void {{}}
    private function secret(): void {{}}

    public function buildForm(array $form, $form_state): array {{
        {line}
"#
    )
}

#[tokio::test]
async fn a_form_callback_completes_the_public_methods_of_the_form() {
    let (backend, dir) = site().await;
    for line in [
        "$form['#submit'][] = '::<|>",
        "$form['#submit'][] = '::<|>'",
        "$form['actions']['submit'] = ['#submit' => ['::saveTitle', '::<|>",
        "$form['#validate'] = ['::<|>'];",
        "$form['x']['#ajax']['callback'] = '::<|>';",
        "$form['x'] = ['#ajax' => ['callback' => '::<|>']];",
        "$form['x'] = ['#process' => [\n            '::<|>',\n        ]];",
        "$form['#submit'] = ['acme_save' => '::<|>'];",
        "$form['#entity_builders']['acme_entity'] = '::<|>';",
    ] {
        let names = completed_names(&backend, &dir, EDITED_FORM_PATH, &edited_form(line), 0).await;
        assert_eq!(names, ["buildForm", "processTitle", "saveTitle"], "{line}");
    }
    let names = completed_names(
        &backend,
        &dir,
        EDITED_FORM_PATH,
        &edited_form("$form['#submit'][] = '::sa<|>"),
        2,
    )
    .await;
    assert_eq!(names, ["saveTitle"]);
}

#[tokio::test]
async fn a_form_callback_reads_the_methods_declared_after_the_cursor() {
    let (backend, dir) = site().await;
    let buffer = FORM.replacen("'::checkTitle';", "'::<|>'", 1);
    let names = completed_names(&backend, &dir, FORM_PATH, &buffer, 0).await;
    assert_eq!(
        names,
        [
            "buildForm",
            "checkTitle",
            "processTitle",
            "refresh",
            "saveTitle"
        ]
    );
}

#[tokio::test]
async fn a_form_callback_completes_a_traits_methods_and_nothing_outside_a_form() {
    let (backend, dir) = site().await;
    let buffer = TRAIT.replacen("'::pressButton';", "'::<|>", 1);
    let names = completed_names(&backend, &dir, TRAIT_PATH, &buffer, 0).await;
    assert_eq!(names, ["addButtons", "pressButton"]);

    let buffer = HOOKS.replacen("'::validate';", "'::<|>", 1);
    assert_eq!(completed(&backend, &dir, HOOKS_PATH, &buffer).await, []);
}

#[tokio::test]
async fn a_form_callback_completes_nothing_where_drupal_does_not_rewrite_it() {
    let (backend, dir) = site().await;
    for line in [
        "$form['#pre_render'][] = '::<|>",
        "$form['x'] = ['#post_render' => ['::<|>",
        "$form['x'] = ['#lazy_builder' => ['::<|>",
        "$form['#access_callback'] = '::<|>",
        "$form['#value_callback'] = '::<|>",
        "$form['#markup'] = '::<|>",
        "$form['x'] = ['callback' => '::<|>",
        "$label = '::<|>",
        "$form['#submit'][] .= '::<|>",
        "$form['#ajax']['wrapper'] = '::<|>",
        "$form['x']['acme_entity'] = '::<|>",
        "$form['x'] = ['#lazy_builder' => ['acme_builder' => '::<|>",
        "$form['x'] = ['#ajax' => ['wrapper' => '::<|>",
    ] {
        let found = completed(&backend, &dir, EDITED_FORM_PATH, &edited_form(line)).await;
        assert_eq!(found, [], "{line}");
    }
}

/// A caller in another namespace than [`HELPER`], which imports no
/// `Helper`.
const RENDER_EDIT_PATH: &str = "web/modules/custom/acme/src/Render/Edited.php";

fn edited_render(line: &str) -> String {
    format!("<?php\nnamespace Drupal\\acme\\Render;\n\nfunction build(): array {{\n    {line}\n")
}

#[tokio::test]
async fn a_class_callback_completes_the_public_methods_of_the_class_it_spells_out() {
    let (backend, dir) = site().await;
    for (path, buffer, expected) in [
        // The file's import of `Drupal\acme\Helper` does not apply.
        (
            EDITED_FORM_PATH,
            edited_form("$form['#submit'][] = 'Helper::<|>"),
            &["shout"][..],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['#submit'][] = 'Drupal\\acme\\Helper::<|>"),
            &["shout", "whisper"],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['#submit'] = ['\\Drupal\\acme\\Helper::<|>"),
            &["shout", "whisper"],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['#submit'][] = \"Drupal\\\\acme\\\\Helper::<|>"),
            &["shout", "whisper"],
        ),
        (
            RENDER_EDIT_PATH,
            edited_render("return ['#pre_render' => ['Helper::<|>"),
            &["shout"],
        ),
        (
            RENDER_EDIT_PATH,
            edited_render("return ['#lazy_builder' => ['Drupal\\acme\\Helper::<|>"),
            &["shout", "whisper"],
        ),
        // A value callback must be a native callable: a static method.
        (
            RENDER_EDIT_PATH,
            edited_render("return ['#value_callback' => 'Drupal\\acme\\Helper::<|>"),
            &[],
        ),
        (
            RENDER_EDIT_PATH,
            edited_render("return ['#value_callback' => 'Helper::<|>"),
            &["shout"],
        ),
        (
            RENDER_EDIT_PATH,
            edited_render("return ['#pre_render' => ['Drupal\\acme\\Missing::<|>"),
            &[],
        ),
    ] {
        let names = completed_names(&backend, &dir, path, &buffer, 0).await;
        assert_eq!(names, expected, "{buffer}");
    }
    let names = completed_names(
        &backend,
        &dir,
        RENDER_EDIT_PATH,
        &edited_render("return ['#pre_render' => ['Drupal\\acme\\Helper::WH<|>"),
        2,
    )
    .await;
    assert_eq!(names, ["whisper"]);
}

#[tokio::test]
async fn a_class_joined_to_a_callback_string_completes_its_methods() {
    let (backend, dir) = site().await;
    for (path, buffer, expected) in [
        // A class name in code follows the file's imports.
        (
            EDITED_FORM_PATH,
            edited_form("$form['#submit'][] = Helper::class . '::<|>"),
            &["shout", "whisper"][..],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['#submit'] = [static::class . '::<|>"),
            &["buildForm", "processTitle", "saveTitle"],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['x']['#ajax']['callback'] = __CLASS__ . '::<|>"),
            &["buildForm", "processTitle", "saveTitle"],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['#value_callback'] = self::class . '::<|>"),
            &["processTitle"],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['#markup'] = static::class . '::<|>"),
            &[],
        ),
        (
            EDITED_FORM_PATH,
            edited_form("$form['#submit'][] = $form_state::class . '::<|>"),
            &[],
        ),
        // Joined to its class, a method of a class that is no form is named
        // directly.
        (
            HOOKS_PATH,
            HOOKS.replacen("'::validate';", "static::class . '::<|>';", 1),
            &["formAlter", "validate"],
        ),
    ] {
        let names = completed_names(&backend, &dir, path, &buffer, 0).await;
        assert_eq!(names, expected, "{buffer}");
    }
}

#[tokio::test]
async fn a_function_callback_completes_function_names_as_written_in_a_string() {
    let (backend, dir) = site().await;
    for (line, typed) in [
        ("$form['#submit'][] = 'acme_log<|>", 8),
        ("$form['#submit'] = ['::saveTitle', 'acme_log<|>", 8),
        ("$form['#submit'][] = '\\acme_log<|>", 8),
        ("$form['x'] = ['#lazy_builder' => ['acme_log<|>", 8),
        ("$form['#value_callback'] = 'acme_log<|>", 8),
    ] {
        let names =
            completed_names(&backend, &dir, EDITED_FORM_PATH, &edited_form(line), typed).await;
        assert_eq!(
            names,
            ["Drupal\\acme\\Form\\acme_log_submit", "acme_log_submit"],
            "{line}"
        );
    }

    // A double-quoted string spells a backslash escaped, both in what it
    // inserts and in what was typed.
    for (line, typed, expected) in [
        (
            "$form['#submit'][] = \"acme_log<|>",
            8,
            &[
                "Drupal\\\\acme\\\\Form\\\\acme_log_submit",
                "acme_log_submit",
            ][..],
        ),
        (
            "$form['#submit'][] = \"Drupal\\\\acme\\\\Form\\\\acme_log<|>",
            28,
            &["Drupal\\\\acme\\\\Form\\\\acme_log_submit"],
        ),
    ] {
        let names =
            completed_names(&backend, &dir, EDITED_FORM_PATH, &edited_form(line), typed).await;
        assert_eq!(names, expected, "{line}");
    }

    // A namespaced function is listed by its full name.
    let content = edited_form("$form['#submit'][] = 'acme_log");
    let uri = uri_for(&dir, EDITED_FORM_PATH);
    crate::common::open_document(&backend, &uri, "php", &content).await;
    let position = crate::common::offset_position(&content, content.len() - 1);
    let labels = crate::common::response_labels(
        crate::common::completion_response(&backend, &uri, position.line, position.character, None)
            .await,
    );
    assert!(
        labels
            .iter()
            .any(|label| label == "Drupal\\acme\\Form\\acme_log_submit"),
        "{labels:?}"
    );
}

#[tokio::test]
async fn only_the_callback_itself_completes() {
    let (backend, dir) = site().await;
    for line in [
        "$form['x'] = ['#lazy_builder' => ['acme_log_submit', ['acme_log<|>",
        "$form['x'] = ['#lazy_builder' => ['acme_log_submit', 'acme_log<|>",
        "$form['x'] = ['#markup' => 'acme_log<|>",
        "$form['x'] = ['callback' => 'acme_log<|>",
        "$form['#submit'] = 'acme_log<|>",
    ] {
        let found = completed(&backend, &dir, EDITED_FORM_PATH, &edited_form(line)).await;
        assert_eq!(found, [], "{line}");
    }

    // The method string of an array callable is left to the array callable.
    let found = completed(
        &backend,
        &dir,
        EDITED_FORM_PATH,
        &edited_form("$form['#submit'] = [[$this, '<|>"),
    )
    .await;
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.contains(&"saveTitle"), "{names:?}");
    assert!(!names.contains(&"acme_log_submit"), "{names:?}");
}

const SERVICE_EDIT_PATH: &str = "web/modules/custom/acme/src/Render/EditedCard.php";

#[tokio::test]
async fn a_service_callback_completes_the_public_methods_of_the_service_class() {
    let (backend, dir) = service_site(false).await;
    let card = |line: &str| {
        format!("<?php\nnamespace Drupal\\acme\\Render;\n\nfunction card(): array {{\n    {line}\n")
    };
    for (line, expected) in [
        (
            "return ['#lazy_builder' => ['acme.greeting_builder:<|>",
            &["build", "preRender"][..],
        ),
        (
            "return ['#pre_render' => ['acme.greeting_alias:pre<|>",
            &["preRender"],
        ),
        (
            "return ['#value_callback' => 'acme.greeting_builder:<|>",
            &[],
        ),
        ("return ['#pre_render' => ['acme.missing:<|>", &[]),
    ] {
        let typed = line.len() - line.find(':').unwrap() - 1 - CURSOR.len();
        let names = completed_names(&backend, &dir, SERVICE_EDIT_PATH, &card(line), typed).await;
        assert_eq!(names, expected, "{line}");
    }
}
