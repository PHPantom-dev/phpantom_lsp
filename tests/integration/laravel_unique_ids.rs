use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER: &str = r#"{
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Illuminate\\Database\\Eloquent\\": "vendor/illuminate/Eloquent/"
    }}
}"#;

const MODEL: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
/** @property int $id */
class Model { protected $keyType = 'int'; }
"#;

const UUIDS: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Concerns;
trait HasUuids {}
"#;

const ULIDS: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Concerns;
trait HasUlids {}
"#;

const WRAPPER: &str = r#"<?php
namespace App\Concerns;
use Illuminate\Database\Eloquent\Concerns\HasUlids as Identifiers;
trait NestedIds { use Identifiers; }
trait Identified { use NestedIds; }
"#;

const PARENT: &str = r#"<?php
namespace App\Models;
use App\Concerns\Identified;
use Illuminate\Database\Eloquent\Model;
class BaseModel extends Model { use Identified; }
"#;

#[tokio::test]
async fn unique_ids_reach_completion_hover_and_diagnostics() {
    for (declaration, key, expected, wrong) in [
        (
            r"use Illuminate\Database\Eloquent\Concerns\HasUuids as Uuids;
class Example extends Model { use Uuids; }",
            "id",
            "string",
            "int",
        ),
        (
            r"class Example extends Model {
    use \Illuminate\Database\Eloquent\Concerns\HasUlids;
}",
            "id",
            "string",
            "int",
        ),
        (r"class Example extends BaseModel {}", "id", "string", "int"),
        (
            r"class Example extends BaseModel { protected $primaryKey = 'identifier'; }",
            "identifier",
            "string",
            "int",
        ),
        (
            r"trait HasUuids {}
class Example extends Model { use HasUuids; }",
            "id",
            "int",
            "string",
        ),
        (
            r"class Example extends BaseModel { public int $id = 1; }",
            "id",
            "int",
            "string",
        ),
    ] {
        let model = format!(
            "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\n{declaration}\n"
        );
        let (backend, dir) = create_psr4_workspace(
            COMPOSER,
            &[
                ("vendor/illuminate/Eloquent/Model.php", MODEL),
                ("vendor/illuminate/Eloquent/Concerns/HasUuids.php", UUIDS),
                ("vendor/illuminate/Eloquent/Concerns/HasUlids.php", ULIDS),
                ("src/Concerns/Identified.php", WRAPPER),
                ("src/Models/BaseModel.php", PARENT),
                ("src/Models/Example.php", &model),
            ],
        );
        let content = format!(
            "<?php declare(strict_types=1);\nuse App\\Models\\Example;\nfunction accepts(Example $model): {expected} {{ return $model->{key}; }}\nfunction rejects(Example $model): {wrong} {{ return $model->{key}; }}\n$model = new Example();\n$model->{key};\n"
        );
        let uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
        open_php(&backend, &uri, &content).await;

        let mut diagnostics = Vec::new();
        backend.collect_slow_diagnostics(uri.as_str(), &content, &mut diagnostics);
        assert_eq!(diagnostics.len(), 1, "{declaration}: {diagnostics:?}");
        assert_eq!(diagnostics[0].range.start.line, 3);
        assert_eq!(
            diagnostics[0].code,
            Some(NumberOrString::String("type_mismatch_return".into()))
        );
        assert_eq!(
            diagnostics[0].message,
            format!("Return type {expected} is incompatible with declared return type {wrong}")
        );

        let position = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(5, 9),
        };
        let hover = backend
            .hover(HoverParams {
                text_document_position_params: position.clone(),
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .expect("primary-key hover");
        let HoverContents::Markup(hover) = hover.contents else {
            panic!("expected markup hover")
        };
        assert!(
            hover.value.contains(&format!("`{expected}`"))
                || hover.value.contains(&format!("{expected} ${key}")),
            "{declaration}: {}",
            hover.value
        );

        let completion = backend
            .completion(CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    position: Position::new(5, 8),
                    ..position
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            })
            .await
            .unwrap()
            .expect("model completion");
        let items = match completion {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let keys: Vec<_> = items.iter().filter(|item| item.label == key).collect();
        assert_eq!(keys.len(), 1, "{declaration}: {items:?}");
        assert!(
            keys[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains(expected),
            "{:?}",
            keys[0]
        );
    }
}

#[tokio::test]
async fn unique_ids_refresh_when_an_inherited_trait_changes() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("vendor/illuminate/Eloquent/Model.php", MODEL),
            ("vendor/illuminate/Eloquent/Concerns/HasUlids.php", ULIDS),
            ("src/Concerns/Identified.php", WRAPPER),
            ("src/Models/BaseModel.php", PARENT),
            (
                "src/Models/Example.php",
                "<?php\nnamespace App\\Models;\nclass Example extends BaseModel {}\n",
            ),
        ],
    );
    let uri = Url::from_file_path(dir.path().join("src/usage.php")).unwrap();
    let content = "<?php declare(strict_types=1);\nfunction keyOf(\\App\\Models\\Example $model): string { return $model->id; }\n";
    open_php(&backend, &uri, content).await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let trait_uri = Url::from_file_path(dir.path().join("src/Concerns/Identified.php")).unwrap();
    open_php(
        &backend,
        &trait_uri,
        "<?php\nnamespace App\\Concerns;\ntrait Identified {}\n",
    )
    .await;
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diagnostics);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(
        diagnostics[0].message,
        "Return type int is incompatible with declared return type string"
    );
}
