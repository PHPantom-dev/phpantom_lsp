//! Column-name arguments in the PHP attributes on Eloquent model classes.

use crate::common::{complete_at, create_psr4_workspace, labels, position_after};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^12.0" },
    "autoload": { "psr-4": {
        "App\\": "app/",
        "Illuminate\\Database\\Eloquent\\": "framework/"
    } }
}"#;

const BASE_MODEL: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Casts\Attribute;
/** @property string $reference */
class BaseUser extends Model {
    protected $fillable = ['name', 'email'];
    protected $casts = ['rank' => 'integer'];
    protected $hidden = ['secret'];
    public string $ordinaryProperty = '';
    protected function displayName(): Attribute {
        return Attribute::make(get: fn (): string => 'Display');
    }
    public function getLegacyNameAttribute(): string { return 'Legacy'; }
}
"#;

async fn complete(marked: &str, installed: bool) -> Vec<CompletionItem> {
    let (before, after) = marked.split_once('|').expect("completion marker");
    let source = format!("{before}{after}");
    let position = position_after(&source, before);
    let attributes: Vec<_> = [
        "Fillable", "Guarded", "Hidden", "Visible", "Appends", "RouteKey",
    ]
    .into_iter()
    .map(|name| {
        let parameters = if name == "RouteKey" {
            "public string $key"
        } else {
            "array|string ...$columns"
        };
        (
            format!("framework/Attributes/{name}.php"),
            format!(
                "<?php namespace Illuminate\\Database\\Eloquent\\Attributes; \
                     #[\\Attribute(\\Attribute::TARGET_CLASS)] \
                     class {name} {{ public function __construct({parameters}) {{}} }}"
            ),
        )
    })
    .collect();
    let mut files = vec![
        (
            "framework/Model.php",
            "<?php namespace Illuminate\\Database\\Eloquent; class Model {}",
        ),
        (
            "framework/Casts/Attribute.php",
            "<?php namespace Illuminate\\Database\\Eloquent\\Casts; class Attribute { public static function make(?callable $get = null): static { return new static; } }",
        ),
        ("app/Models/BaseUser.php", BASE_MODEL),
        (
            "app/Fillable.php",
            "<?php namespace App; class Fillable { public function __construct(array $columns) {} }",
        ),
        ("app/Models/User.php", source.as_str()),
    ];
    if installed {
        files.extend(
            attributes
                .iter()
                .map(|(path, text)| (path.as_str(), text.as_str())),
        );
    }
    let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(dir.path().join("app/Models/User.php")).unwrap();
    complete_at(&backend, &uri, &source, position.line, position.character).await
}

fn model(attribute: &str) -> String {
    format!(
        "<?php\nnamespace App\\Models;\n\
         use Illuminate\\Database\\Eloquent\\Attributes\\{{Fillable, Guarded, Hidden, Visible, Appends, RouteKey}};\n\
         {attribute}\nclass User extends BaseUser {{}}\n"
    )
}

#[tokio::test]
async fn eloquent_model_attributes_complete_installed_column_lists() {
    for attribute in ["Fillable", "Guarded", "Hidden", "Visible", "Appends"] {
        for arguments in [
            "['na|']",
            "'email', 'na|'",
            "array(/* columns */ 'na|')",
            "[0 => 'na|']",
        ] {
            let source = model(&format!("#[{attribute}({arguments})]"));
            let items = complete(&source, true).await;
            assert_eq!(labels(&items), vec!["name"], "{source}");
            assert_eq!(items[0].kind, Some(CompletionItemKind::Field));
        }
    }
}

#[tokio::test]
async fn eloquent_model_attributes_include_inherited_column_sources() {
    let items = complete(&model("#[Hidden(['|'])]"), true).await;
    let names = labels(&items);
    for expected in [
        "name",
        "email",
        "rank",
        "secret",
        "reference",
        "display_name",
        "legacy_name",
    ] {
        assert!(names.contains(&expected), "missing {expected}: {names:?}");
    }
    for excluded in ["ordinaryProperty", "fillable", "casts", "hidden"] {
        assert!(
            !names.contains(&excluded),
            "unexpected {excluded}: {names:?}"
        );
    }
}

#[tokio::test]
async fn eloquent_model_attributes_resolve_aliases_and_qualified_names() {
    for (imports, attribute) in [
        (
            "use Illuminate\\Database\\Eloquent\\Attributes\\Fillable as MassAssignable;",
            "#[MassAssignable(['na|'])]",
        ),
        (
            "use Illuminate\\Database\\Eloquent\\Attributes as ModelAttributes;",
            "#[ModelAttributes\\Visible(['na|'])]",
        ),
        (
            "",
            "#[\\Illuminate\\Database\\Eloquent\\Attributes\\Guarded(['na|'])]",
        ),
        (
            "use Illuminate\\Database\\Eloquent\\Attributes\\Fillable as 可填写;",
            "#[可填写(['na|'])]",
        ),
    ] {
        let source = format!(
            "<?php namespace App\\Models; {imports}\n{attribute}\nclass User extends BaseUser {{}}"
        );
        let items = complete(&source, true).await;
        assert_eq!(labels(&items), vec!["name"], "{source}");
    }
}

#[tokio::test]
async fn eloquent_model_attributes_use_namespace_local_imports() {
    let source = r#"<?php
namespace Unrelated {
    use App\Fillable;
    #[Fillable(['other'])]
    class Other {}
}
namespace App\Models {
    use Illuminate\Database\Eloquent\Attributes\Fillable;
    #[Fillable(['na|'])]
    class User extends BaseUser {}
}
"#;
    assert_eq!(labels(&complete(source, true).await), vec!["name"]);
}

#[tokio::test]
async fn eloquent_route_key_attribute_accepts_positional_and_named_key() {
    for arguments in ["'na|'", "key: /* key column */ 'na|'"] {
        let source = model(&format!("#[RouteKey({arguments})]"));
        assert_eq!(
            labels(&complete(&source, true).await),
            vec!["name"],
            "{source}"
        );
    }
}

#[tokio::test]
async fn eloquent_model_attribute_completion_replaces_literal_and_skips_siblings() {
    let marked = model("#[Visible(['email', 'na|me', 'rank'])]");
    let items = complete(&marked, true).await;
    assert_eq!(labels(&items), vec!["name"]);
    let source = marked.replacen('|', "", 1);
    let Some(CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit else {
        panic!("column completion must replace the literal contents");
    };
    assert_eq!(edit.new_text, "name");
    assert_eq!(edit.range.start, position_after(&source, "'email', '"));
    assert_eq!(edit.range.end, position_after(&source, "'email', 'name"));

    let items = complete(&model("#[Visible(['email', '|', 'rank'])]"), true).await;
    let names = labels(&items);
    assert!(names.contains(&"name"), "{names:?}");
    assert!(!names.contains(&"email"), "{names:?}");
    assert!(!names.contains(&"rank"), "{names:?}");
}

#[tokio::test]
async fn eloquent_model_attributes_require_supported_column_positions() {
    for attribute in [
        "#[Fillable(['na|' => 'email'])]",
        "#[Fillable([['na|']])]",
        "#[Fillable('email', ['na|'])]",
        "#[Fillable(['email'], 'na|')]",
        "#[Fillable(columns: ['na|'])]",
        "#[Fillable('email' . 'na|')]",
        "#[RouteKey(['na|'])]",
        "#[RouteKey('email', 'na|')]",
        "#[RouteKey(other: 'na|')]",
        "#[\\Illuminate\\Database\\Eloquent\\Attributes\\Touches(['na|'])]",
        "#[\\Illuminate\\Database\\Eloquent\\Attributes\\Table('na|')]",
    ] {
        let source = model(attribute);
        let items = complete(&source, true).await;
        assert!(
            !labels(&items).contains(&"name"),
            "{source}: {:?}",
            labels(&items)
        );
    }
}

#[tokio::test]
async fn eloquent_model_attributes_require_installed_framework_attribute() {
    let source = model("#[Fillable(['na|'])]");
    assert!(!labels(&complete(&source, false).await).contains(&"name"));

    let source = "<?php namespace App\\Models; use App\\Fillable;\n\
        #[Fillable(['na|'])] class User extends BaseUser {}";
    assert!(!labels(&complete(source, true).await).contains(&"name"));

    let source = r#"<?php
namespace { class Fillable {} }
namespace App\Models {
    use Illuminate\Database\Eloquent\Attributes\Fillable;
    #[Fillable(['na|'])]
    class User extends BaseUser {}
}
"#;
    assert!(!labels(&complete(source, false).await).contains(&"name"));
}

#[tokio::test]
async fn eloquent_model_attributes_require_a_model_class_target() {
    for declaration in [
        "#[Fillable(['na|'])] class User { protected $fillable = ['name']; }",
        "class User extends BaseUser { #[Fillable(['na|'])] public function example() {} }",
        "class User extends BaseUser { #[Fillable(['na|'])] public string $example; }",
        "#[Fillable(['na|'])] function example() {} class User extends BaseUser {}",
        "// #[Fillable(['na|'])]\nclass User extends BaseUser {}",
    ] {
        let source = model("").replace("class User extends BaseUser {}", declaration);
        let items = complete(&source, true).await;
        assert!(
            !labels(&items).contains(&"name"),
            "{source}: {:?}",
            labels(&items)
        );
    }
}

#[tokio::test]
async fn eloquent_model_attributes_find_the_annotated_model_in_grouped_attributes() {
    let source =
        model("#[RouteKey('id'), Fillable( // first column\n ['na|'])]\n#[Hidden(['secret'])]");
    assert_eq!(labels(&complete(&source, true).await), vec!["name"]);
}
