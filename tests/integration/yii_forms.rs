//! Yii form fields and hydration maps: the property a form field names,
//! and the keys of the map a form hydrator or an `ArrayData` is given,
//! navigate to the property, complete from the object's properties, rename
//! with it, and are reported when the object has no such property.
use crate::common::{
    completion_response, edits_for_uri, initialize_with_options, message_text, offset_position,
    open_document, rename, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/form-model": "^1.0",
        "yiisoft/hydrator": "^1.6"
    },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Yiisoft\\FormModel\\": "stubs/form-model/",
            "Yiisoft\\Hydrator\\": "stubs/hydrator/"
        }
    }
}"#;

const FORM_MODEL_INTERFACE_PHP: &str = r#"<?php
namespace Yiisoft\FormModel;

interface FormModelInterface {}
"#;

const FORM_MODEL_PHP: &str = r#"<?php
namespace Yiisoft\FormModel;

abstract class FormModel implements FormModelInterface {}
"#;

const FIELD_PHP: &str = r#"<?php
namespace Yiisoft\FormModel;

class Field
{
    final public static function text(FormModelInterface $formModel, string $property, array $config = []): string { return ''; }
    final public static function label(FormModelInterface $formModel, string $property): string { return ''; }
}
"#;

const FIELD_FACTORY_PHP: &str = r#"<?php
namespace Yiisoft\FormModel;

class FieldFactory
{
    final public function text(FormModelInterface $formModel, string $property, array $config = []): string { return ''; }
    final public function select(FormModelInterface $formModel, string $property): string { return ''; }
}
"#;

const FORM_HYDRATOR_PHP: &str = r#"<?php
namespace Yiisoft\FormModel;

final class FormHydrator
{
    public function populate(FormModelInterface $model, mixed $data, ?array $map = null, ?bool $strict = null): bool { return true; }
}
"#;

const HYDRATOR_PHP: &str = r#"<?php
namespace Yiisoft\Hydrator;

final class Hydrator
{
    public function hydrate(object $object, array|ArrayData $data = []): void {}
}
"#;

const ARRAY_DATA_PHP: &str = r#"<?php
namespace Yiisoft\Hydrator;

final class ArrayData
{
    public function __construct(array $data = [], array|ObjectMap $map = [], bool $strict = false) {}
}
"#;

const OBJECT_MAP_PHP: &str = r#"<?php
namespace Yiisoft\Hydrator;

final class ObjectMap
{
    public function __construct(public readonly array $map) {}
}
"#;

const POST_FORM_PHP: &str = r#"<?php
namespace App\Form;

use Yiisoft\FormModel\FormModel;

final class PostForm extends FormModel
{
    public string $title = '';
    private string $body = '';
    public array $tags = [];
    public static string $shared = '';

    public function summary(): string { return ''; }
}
"#;

const POST_PHP: &str = r#"<?php
namespace App\Domain;

final class Post
{
    public string $title = '';
    private string $slug = '';
}
"#;

/// Not a form model, so the strings it is given are its own business.
const WIDGET_PHP: &str = r#"<?php
namespace App\Form;

final class Widget
{
    public function text(object $owner, string $label): string { return ''; }
}
"#;

const TEMPLATE_PHP: &str = r#"<?php
use App\Form\PostForm;
use App\Form\Widget;
use Yiisoft\FormModel\Field;
use Yiisoft\FormModel\FieldFactory;

/**
 * @var PostForm $form
 * @var Widget $widget
 */

$field = new FieldFactory();
echo Field::text($form, 'title');
echo $field->text($form, 'body');
echo $field->select($form, 'tags[0]');
echo Field::label($form, '[0]title');
echo Field::text($form, 'titel');
echo Field::text($form, 'summary');
echo $widget->text($widget, 'caption');
echo $field->text(property: 'title', formModel: $form);
"#;

const ACTION_PHP: &str = r#"<?php
namespace App\Action;

use App\Domain\Post;
use App\Form\PostForm;
use Yiisoft\FormModel\FormHydrator;
use Yiisoft\Hydrator\ArrayData;
use Yiisoft\Hydrator\Hydrator;
use Yiisoft\Hydrator\ObjectMap;

final class SaveAction
{
    public function __invoke(FormHydrator $forms, Hydrator $hydrator, PostForm $form, Post $post, array $data): void
    {
        $forms->populate($form, $data, ['title' => 'post_title', 'bdy' => 'text']);
        $forms->populate($form, $data, map: ['body' => 'text']);
        $hydrator->hydrate($post, new ArrayData($data, ['title' => 'name', 'slug' => 'handle', 'sulg' => 'x']));
        $hydrator->hydrate($post, new ArrayData($data, new ObjectMap(['title' => 'name'])));
        $other = new ArrayData($data, ['anything' => 'goes']);
    }
}
"#;

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("composer.json", COMPOSER),
        (
            "stubs/form-model/FormModelInterface.php",
            FORM_MODEL_INTERFACE_PHP,
        ),
        ("stubs/form-model/FormModel.php", FORM_MODEL_PHP),
        ("stubs/form-model/Field.php", FIELD_PHP),
        ("stubs/form-model/FieldFactory.php", FIELD_FACTORY_PHP),
        ("stubs/form-model/FormHydrator.php", FORM_HYDRATOR_PHP),
        ("stubs/hydrator/Hydrator.php", HYDRATOR_PHP),
        ("stubs/hydrator/ArrayData.php", ARRAY_DATA_PHP),
        ("stubs/hydrator/ObjectMap.php", OBJECT_MAP_PHP),
        ("src/Form/PostForm.php", POST_FORM_PHP),
        ("src/Form/Widget.php", WIDGET_PHP),
        ("src/Domain/Post.php", POST_PHP),
        ("src/Action/SaveAction.php", ACTION_PHP),
        ("resources/views/post/form.php", TEMPLATE_PHP),
    ]
}

/// A session over the workspace with its PHP files open.
async fn workspace() -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in files() {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
    let backend = Backend::new_test();
    initialize_with_options(&backend, dir.path(), None).await;
    for (path, content) in files() {
        if path.ends_with(".php") && !path.starts_with("stubs/") {
            open_document(&backend, &uri_for(&dir, path), "php", content).await;
        }
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dunce::canonicalize(dir.path()).unwrap().join(rel)).unwrap()
}

/// The position `inside` bytes into the first occurrence of `needle`.
fn position_in(content: &str, needle: &str, inside: usize) -> Position {
    offset_position(
        content,
        content.find(needle).expect("needle should exist") + inside,
    )
}

/// Where go-to-definition from `inside` bytes into `needle` in the file
/// at `rel` lands.
async fn definition_of(
    backend: &Backend,
    dir: &tempfile::TempDir,
    rel: &str,
    content: &str,
    needle: &str,
    inside: usize,
) -> Option<Location> {
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: uri_for(dir, rel),
                },
                position: position_in(content, needle, inside),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match response {
        Some(DefinitionResponse::Definition(Definition::Location(location))) => Some(location),
        Some(DefinitionResponse::Definition(Definition::LocationList(mut list))) => list.pop(),
        _ => None,
    }
}

/// The diagnostics with `code` the file at `rel` gets, as their messages
/// and the text they cover.
fn reported(
    backend: &Backend,
    dir: &tempfile::TempDir,
    rel: &str,
    content: &str,
    code: &str,
) -> Vec<(String, String)> {
    let uri = uri_for(dir, rel);
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    let lines: Vec<&str> = content.lines().collect();
    diagnostics
        .iter()
        .filter(|diagnostic| matches!(&diagnostic.code, Some(Code::String(found)) if found == code))
        .map(|diagnostic| {
            let range = diagnostic.range;
            let line = lines[range.start.line as usize];
            let text = &line[range.start.character as usize..range.end.character as usize];
            (
                message_text(&diagnostic.message).to_string(),
                text.to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn a_form_field_names_a_property_of_the_form_model() {
    let (backend, dir) = workspace().await;
    let template = "resources/views/post/form.php";
    let form = uri_for(&dir, "src/Form/PostForm.php");
    let title = position_in(POST_FORM_PHP, "$title", 0).line;

    for (needle, inside) in [
        ("'title'", 2),
        ("'body'", 2),
        ("'tags[0]'", 2),
        ("'[0]title'", 5),
        ("property: 'title'", 12),
    ] {
        let location = definition_of(&backend, &dir, template, TEMPLATE_PHP, needle, inside)
            .await
            .unwrap_or_else(|| panic!("{needle} should navigate"));
        assert_eq!(location.uri, form, "{needle}");
        let expected = if needle.contains("body") {
            position_in(POST_FORM_PHP, "$body", 0).line
        } else if needle.contains("tags") {
            position_in(POST_FORM_PHP, "$tags", 0).line
        } else {
            title
        };
        assert_eq!(location.range.start.line, expected, "{needle}");
    }

    // A string given with something other than a form model names nothing.
    assert_eq!(
        definition_of(&backend, &dir, template, TEMPLATE_PHP, "'caption'", 2).await,
        None
    );
}

#[tokio::test]
async fn a_form_field_naming_no_property_is_reported() {
    let (backend, dir) = workspace().await;
    let template = "resources/views/post/form.php";
    let unknown: Vec<String> = reported(&backend, &dir, template, TEMPLATE_PHP, "unknown_member")
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    // A method is no property to read, whatever its name.
    assert_eq!(unknown, vec!["titel".to_string(), "summary".to_string()]);
    // The form model reads a property by reflection, so a private one is
    // within reach.
    assert_eq!(
        reported(
            &backend,
            &dir,
            template,
            TEMPLATE_PHP,
            "inaccessible_member"
        ),
        Vec::new()
    );
}

#[tokio::test]
async fn a_form_field_completes_the_form_models_properties() {
    let (backend, dir) = workspace().await;
    let uri = uri_for(&dir, "resources/views/post/form.php");

    for (from, to) in [
        ("Field::text($form, 'titel')", "Field::text($form, '')"),
        ("$field->text($form, 'body')", "$field->text($form, '')"),
    ] {
        let typing = TEMPLATE_PHP.replace(from, to);
        open_document(&backend, &uri, "php", &typing).await;
        let at = position_in(&typing, to, to.len() - 2);
        let labels =
            response_labels(completion_response(&backend, &uri, at.line, at.character, None).await);
        assert_eq!(labels, vec!["title", "body", "tags"], "{to}");
    }

    let typing = TEMPLATE_PHP.replace("'caption'", "''");
    open_document(&backend, &uri, "php", &typing).await;
    let at = position_in(&typing, "$widget->text($widget, ''", 24);
    let labels =
        response_labels(completion_response(&backend, &uri, at.line, at.character, None).await);
    assert!(!labels.iter().any(|label| label == "title"), "{labels:?}");
}

#[tokio::test]
async fn a_hydration_map_is_keyed_by_the_objects_properties() {
    let (backend, dir) = workspace().await;
    let action = "src/Action/SaveAction.php";
    let form = uri_for(&dir, "src/Form/PostForm.php");
    let post = uri_for(&dir, "src/Domain/Post.php");

    let location = definition_of(&backend, &dir, action, ACTION_PHP, "'title' => 'post", 2)
        .await
        .expect("a form hydrator's map key should navigate");
    assert_eq!(location.uri, form);
    let location = definition_of(&backend, &dir, action, ACTION_PHP, "'slug'", 2)
        .await
        .expect("an ArrayData map key should navigate");
    assert_eq!(location.uri, post);
    assert_eq!(
        location.range.start.line,
        position_in(POST_PHP, "$slug", 0).line
    );
    let location = definition_of(&backend, &dir, action, ACTION_PHP, "ObjectMap(['title'", 13)
        .await
        .expect("an ObjectMap key should navigate");
    assert_eq!(location.uri, post);
    // An `ArrayData` no hydrator is given fills no object we know of.
    assert_eq!(
        definition_of(&backend, &dir, action, ACTION_PHP, "'anything'", 2).await,
        None
    );

    let unknown: Vec<String> = reported(&backend, &dir, action, ACTION_PHP, "unknown_member")
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    assert_eq!(unknown, vec!["bdy".to_string(), "sulg".to_string()]);
    assert_eq!(
        reported(&backend, &dir, action, ACTION_PHP, "inaccessible_member"),
        Vec::new()
    );
}

#[tokio::test]
async fn a_hydration_map_completes_the_objects_properties() {
    let (backend, dir) = workspace().await;
    let uri = uri_for(&dir, "src/Action/SaveAction.php");

    let typing = ACTION_PHP.replace("'bdy' => 'text'", "''");
    open_document(&backend, &uri, "php", &typing).await;
    let at = position_in(&typing, "'post_title', ''", 15);
    let labels =
        response_labels(completion_response(&backend, &uri, at.line, at.character, None).await);
    assert_eq!(labels, vec!["body", "tags"]);

    let typing = ACTION_PHP.replace("'sulg' => 'x'", "''");
    open_document(&backend, &uri, "php", &typing).await;
    let at = position_in(&typing, "'handle', ''", 11);
    let labels =
        response_labels(completion_response(&backend, &uri, at.line, at.character, None).await);
    assert!(labels.is_empty(), "{labels:?}");

    let typing = ACTION_PHP.replace("ObjectMap(['title' => 'name'])", "ObjectMap([''])");
    open_document(&backend, &uri, "php", &typing).await;
    let at = position_in(&typing, "ObjectMap([''", 12);
    let labels =
        response_labels(completion_response(&backend, &uri, at.line, at.character, None).await);
    assert_eq!(labels, vec!["title", "slug"]);
}

#[tokio::test]
async fn renaming_a_property_renames_the_fields_and_map_keys() {
    let (backend, dir) = workspace().await;
    let form = uri_for(&dir, "src/Form/PostForm.php");
    let template = uri_for(&dir, "resources/views/post/form.php");
    let action = uri_for(&dir, "src/Action/SaveAction.php");

    let at = position_in(POST_FORM_PHP, "$title", 1);
    let edit = rename(&backend, &form, at.line, at.character, "heading")
        .await
        .expect("rename should produce an edit");
    let mut renamed: Vec<Position> = edits_for_uri(&edit, &template)
        .iter()
        .map(|edit| edit.range.start)
        .collect();
    renamed.sort_by_key(|position| (position.line, position.character));
    assert_eq!(
        renamed,
        vec![
            position_in(TEMPLATE_PHP, "'title'", 1),
            position_in(TEMPLATE_PHP, "'[0]title'", 4),
            position_in(TEMPLATE_PHP, "property: 'title'", 11),
        ]
    );
    let edits = edits_for_uri(&edit, &action);
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(
        edits[0].range.start,
        position_in(ACTION_PHP, "'title' => 'post", 1)
    );
}
