//! Column completions in Eloquent attribute arrays and scalar arguments.

use crate::common::{
    complete_at, create_psr4_workspace, item_labels, position_after, split_cursor,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemTextEdit, Range, Uri,
};

const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^12.0" },
    "autoload": { "psr-4": {
        "App\\": "app/",
        "Illuminate\\Database\\Eloquent\\": "framework/"
    } }
}"#;

const MODEL: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
class Model {
    protected $guarded = ['*'];
    /** @return Builder<static> */
    public static function query() { return new Builder(); }
    /** @return static */
    public function fill(array $attributes) { return $this; }
    public function update(array $attributes = [], array $options = []): bool { return true; }
    /**
     * @template TRelatedModel of Model
     * @param class-string<TRelatedModel> $related
     * @return Relations\HasMany<TRelatedModel, $this>
     */
    public function hasMany($related) {}
}
"#;

const BUILDER: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
/** @template TModel of Model */
class Builder {
    /** @return $this */
    public function where($column, $operator = null, $value = null) { return $this; }
    /** @return TModel */
    public function create(array $attributes = []) {}
    /** @return TModel */
    public function make(array $attributes = []) {}
    public function update(array $values) {}
    /** @return TModel */
    public function updateOrCreate(array $attributes, array $values = []) {}
    /** @return TModel */
    public function firstOrCreate(array $attributes = [], array $values = []) {}
    /** @return TModel */
    public function firstOrNew(array $attributes = [], array $values = []) {}
    /** @return TModel */
    public function createOrFirst(array $attributes = [], array $values = []) {}
    /** @return TModel|null */
    public function firstWhere($column, $operator = null, $value = null) {}
    /** @return $this */
    public function whereColumn($first, $operator = null, $second = null) { return $this; }
    public function min($column) {}
    public function max($column) {}
    public function sum($column) {}
    public function avg($column) {}
}
"#;

const HAS_MANY: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @mixin \Illuminate\Database\Eloquent\Builder<TRelatedModel>
 */
class HasMany {
    /** @return TRelatedModel */
    public function create(array $attributes = []) {}
}
"#;

const ACCOUNT: &str = r#"<?php
namespace App\Models;
class Account extends \Illuminate\Database\Eloquent\Model {
    protected $fillable = ['name', 'email'];
}
"#;

const USER: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Relations\HasMany;
/** @property int $tenant_id */
class User extends Account {
    protected $casts = ['age' => 'integer'];
    protected $attributes = ['status' => 'active'];
    protected $guarded = ['locked'];
    public function getDisplayNameAttribute(): string { return ''; }
    /** @return HasMany<Post, $this> */
    public function posts(): HasMany { return $this->hasMany(Post::class); }
}
"#;

const POST: &str = r#"<?php
namespace App\Models;
class Post extends \Illuminate\Database\Eloquent\Model {
    protected $fillable = ['title', 'body'];
}
"#;

fn workspace() -> (Backend, tempfile::TempDir, Uri) {
    workspace_with_models(ACCOUNT, USER)
}

fn workspace_with_models(account: &str, user: &str) -> (Backend, tempfile::TempDir, Uri) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("framework/Model.php", MODEL),
            ("framework/Builder.php", BUILDER),
            ("framework/Relations/HasMany.php", HAS_MANY),
            (
                "framework/Relations/Pivot.php",
                "<?php namespace Illuminate\\Database\\Eloquent\\Relations; class Pivot extends \\Illuminate\\Database\\Eloquent\\Model { protected $guarded = []; }",
            ),
            ("app/Models/Account.php", account),
            ("app/Models/User.php", user),
            ("app/Models/Post.php", POST),
        ],
    );
    let uri = Uri::from_file_path(dir.path().join("app/Consumer.php")).unwrap();
    (backend, dir, uri)
}

async fn complete_statement(backend: &Backend, uri: &Uri, statement: &str) -> Vec<CompletionItem> {
    let (source, position) = split_cursor(&format!(
        r#"<?php
namespace App;
use App\Models\User;
use Illuminate\Database\Eloquent\Builder;
class Ordinary {{
    public string $name;
    public function create(array $attributes) {{}}
    public function whereColumn($first, $operator = null, $second = null) {{}}
}}
class Consumer {{
    /** @param Builder<User> $query */
    public function run(User $user, Builder $query, Ordinary $ordinary): void {{
        {statement}
    }}
}}
"#
    ));
    complete_at(backend, uri, &source, position.line, position.character).await
}

#[tokio::test]
async fn attribute_arrays_offer_inherited_and_local_column_sources() {
    let (backend, _dir, uri) = workspace();
    let items = complete_statement(&backend, &uri, "User::create(['§' => 'Ada']);").await;
    let labels = item_labels(items);
    for column in [
        "name",
        "email",
        "age",
        "status",
        "locked",
        "tenant_id",
        "display_name",
    ] {
        assert!(
            labels.iter().any(|label| label == column),
            "missing {column}: {labels:?}"
        );
    }
    for non_column in ["fillable", "guarded", "casts", "attributes", "posts"] {
        assert!(
            !labels.iter().any(|label| label == non_column),
            "offered {non_column}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn fillable_columns_sort_before_other_known_attributes() {
    let (backend, _dir, uri) = workspace();
    let items = complete_statement(&backend, &uri, "User::create(['§']);").await;
    let sort_key = |name: &str| {
        let item = items.iter().find(|item| item.label == name).unwrap();
        item.sort_text.as_deref().unwrap_or(&item.label)
    };
    for fillable in ["name", "email"] {
        for other in ["age", "locked", "tenant_id"] {
            assert!(
                sort_key(fillable) < sort_key(other),
                "{fillable} should precede {other}: {items:?}"
            );
        }
    }
}

#[tokio::test]
async fn model_builder_and_instance_attribute_methods_complete() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "User::create(['na§' => 'Ada']);",
        "User::make(['na§' => 'Ada']);",
        "User::query()->create(['na§' => 'Ada']);",
        "User::query()->where('age', 18)->update(['na§' => 'Ada']);",
        "$query->create(['na§' => 'Ada']);",
        "$query->make(['na§' => 'Ada']);",
        "$query->update(['na§' => 'Ada']);",
        "$user->fill(['na§' => 'Ada']);",
        "$user->update(['na§' => 'Ada']);",
        "$user?->fill(['na§' => 'Ada']);",
    ] {
        let items = complete_statement(&backend, &uri, statement).await;
        let labels = item_labels(items);
        assert!(
            labels.iter().any(|label| label == "name"),
            "{statement}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn matching_and_values_arrays_complete_independently() {
    let (backend, _dir, uri) = workspace();
    for method in [
        "updateOrCreate",
        "firstOrCreate",
        "firstOrNew",
        "createOrFirst",
    ] {
        for arguments in [
            "['na§' => 'Ada'], ['name' => 'Grace']",
            "['name' => 'Ada'], ['na§' => 'Grace']",
        ] {
            let statement = format!("User::{method}({arguments});");
            let labels = item_labels(complete_statement(&backend, &uri, &statement).await);
            assert!(
                labels.iter().any(|label| label == "name"),
                "{statement}: {labels:?}"
            );
        }
    }
}

#[tokio::test]
async fn attribute_arrays_support_named_arguments_comments_and_long_syntax() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "User::create(attributes: ['na§' => 'Ada']);",
        "User::create(array('na§' => 'Ada'));",
        "User::CREATE(ARRAY('na§' => 'Ada'));",
        "User::make(attributes: array(/* column */ 'na§' => 'Ada'));",
        "User::updateOrCreate(values: ['na§' => 'Ada'], attributes: ['email' => 'a@example.test']);",
        "$query->update(values: ['na§' => 'Ada']);",
        "$user->update(options: [], attributes: ['na§' => 'Ada']);",
        "$user->fill( /* attributes */ [ /* key */ \"na§\" => 'Ada']);",
    ] {
        let labels = item_labels(complete_statement(&backend, &uri, statement).await);
        assert!(
            labels.iter().any(|label| label == "name"),
            "{statement}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn related_model_attributes_follow_the_relationship_receiver() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "$user->posts()->create(['§' => 'Hello']);",
        "$user->posts()->where('title', 'Hello')->create(['§' => 'World']);",
    ] {
        let labels = item_labels(complete_statement(&backend, &uri, statement).await);
        assert!(
            labels.iter().any(|label| label == "title"),
            "{statement}: {labels:?}"
        );
        for excluded in ["name", "*"] {
            assert!(
                !labels.iter().any(|label| label == excluded),
                "unexpected attribute {excluded}: {labels:?}"
            );
        }
    }
}

#[tokio::test]
async fn existing_literal_keys_before_and_after_the_cursor_are_omitted() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "User::create(['name' => 'Ada', '§' => 10, 'email' => 'a@example.test']);",
        "User::create(array('name' => 'Ada', '§' => 10, 'email' => 'a@example.test'));",
        "User::create(['name' => <<<'TEXT'\n)\nTEXT, '§' => 10, 'email' => 'a@example.test']);",
        "User::create(['name' => 'Ada', '§' => 10, 'email' => <<<TEXT\n)\nTEXT\n]);",
    ] {
        let labels = item_labels(complete_statement(&backend, &uri, statement).await);
        assert!(
            labels.iter().any(|label| label == "age"),
            "{statement}: {labels:?}"
        );
        for present in ["name", "email"] {
            assert!(
                !labels.iter().any(|label| label == present),
                "repeated {present}: {labels:?}"
            );
        }
    }
}

#[tokio::test]
async fn sibling_values_nested_keys_and_the_edited_key_remain_available() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "User::create(['status' => 'name', 'na§' => 'Ada']);",
        "User::create(['metadata' => ['name' => 'Ada'], 'na§' => 'Grace']);",
        "User::create(['NAME' => 'Ada', 'na§' => 'Grace']);",
        "User::create(['na§me' => 'Ada']);",
        "User::create(['status' => fn () => ['name' => 'Ada'], 'na§' => 'Grace']);",
    ] {
        let labels = item_labels(complete_statement(&backend, &uri, statement).await);
        assert!(
            labels.iter().any(|label| label == "name"),
            "{statement}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn unfinished_attribute_keys_complete_without_an_arrow() {
    let (backend, _dir, uri) = workspace();
    for statement in ["User::create(['na§']);", "User::create(['na§"] {
        let items = complete_statement(&backend, &uri, statement).await;
        let item = items
            .iter()
            .find(|item| item.label == "name")
            .unwrap_or_else(|| panic!("{statement}: {items:?}"));
        assert_eq!(item.kind, Some(CompletionItemKind::Field));
        assert_eq!(item.insert_text.as_deref(), Some("name"));
    }
}

#[tokio::test]
async fn attribute_completion_replaces_the_prefix_inside_the_quotes() {
    let (backend, _dir, uri) = workspace();
    let (source, position) =
        split_cursor("<?php\nuse App\\Models\\User;\n/* 😀 */ User::create(['na§' => 'Ada']);");
    let items = complete_at(&backend, &uri, &source, position.line, position.character).await;
    let item = items.iter().find(|item| item.label == "name").unwrap();
    let Some(CompletionItemTextEdit::TextEdit(edit)) = &item.text_edit else {
        panic!("expected an explicit string replacement: {item:?}");
    };
    assert_eq!(edit.new_text, "name");
    assert_eq!(
        edit.range,
        Range::new(position_after(&source, "create(['"), position)
    );
}

#[tokio::test]
async fn array_values_nested_arrays_and_unrelated_arguments_do_not_offer_columns() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "User::create(['name' => 'na§']);",
        "User::create(['metadata' => ['na§' => 'Ada']]);",
        "User::create([['na§' => 'Ada']]);",
        "User::create('na§');",
        "User::create(...['na§' => 'Ada']);",
        "User::create([], ['na§' => 'Ada']);",
        "$user->update([], ['na§' => 'Ada']);",
        "$user->fill(options: ['na§' => 'Ada']);",
        "$query->update(options: ['na§' => 'Ada']);",
        "User::firstOrCreate([], [], ['na§' => 'Ada']);",
        "User::where('age', ['na§' => 'Ada']);",
        "$ordinary->create(['na§' => 'Ada']);",
        "Ordinary::create(['na§' => 'Ada']);",
    ] {
        let labels = item_labels(complete_statement(&backend, &uri, statement).await);
        assert!(
            !labels.iter().any(|label| label == "name"),
            "{statement}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn additional_scalar_column_methods_complete_their_column_arguments() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "User::firstWhere('na§', 'Ada');",
        "User::firstWhere(value: 'Ada', column: 'na§');",
        "$query->whereColumn('na§', '=', 'email');",
        "$query->whereColumn('email', 'na§');",
        "$query->whereColumn('email', '=', 'na§');",
        "$query->whereColumn(second: 'na§', first: 'email');",
        "$query->min('na§');",
        "User::max('na§');",
        "User::query()->sum('na§');",
        "$query->avg(column: 'na§');",
        "User::where(['na§' => 'Ada']);",
        "$query->where(['age' => 18, 'na§' => 'Ada']);",
    ] {
        let labels = item_labels(complete_statement(&backend, &uri, statement).await);
        assert!(
            labels.iter().any(|label| label == "name"),
            "{statement}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn scalar_values_and_where_column_operators_do_not_offer_columns() {
    let (backend, _dir, uri) = workspace();
    for statement in [
        "User::firstWhere('email', 'na§');",
        "User::firstWhere('email', '=', 'na§');",
        "$query->whereColumn('email', 'na§', 'name');",
        "$query->whereColumn(first: 'email', operator: 'na§', second: 'name');",
        "$query->whereColumn(first: 'email', operator: 'na§');",
        "$query->min('email', 'na§');",
        "$ordinary->whereColumn('na§', 'email');",
    ] {
        let labels = item_labels(complete_statement(&backend, &uri, statement).await);
        assert!(
            !labels.iter().any(|label| label == "name"),
            "{statement}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn model_column_attributes_supply_columns_and_fillable_ordering() {
    let user = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Attributes as Columns;
#[Columns\Fillable(['nickname', 'email'])]
#[Columns\Guarded('secret')]
#[Columns\Hidden(array('token'))]
#[Columns\Visible([0 => 'public_name'])]
#[Columns\Appends(['display_name'])]
class User extends Account {
    protected $fillable = ['local'];
    protected $casts = ['nickname' => 'string'];
}
"#;
    let (backend, _dir, uri) = workspace_with_models(ACCOUNT, user);
    for statement in [
        "User::create(['§']);",
        "$user->fill(['§']);",
        "User::where('§', 1);",
    ] {
        let items = complete_statement(&backend, &uri, statement).await;
        for name in [
            "nickname",
            "email",
            "secret",
            "token",
            "public_name",
            "display_name",
            "local",
        ] {
            assert!(
                items.iter().any(|item| item.label == name),
                "missing {name}: {items:?}"
            );
        }
        assert!(!items.iter().any(|item| item.label == "name"));
        if !statement.contains("where") {
            for name in ["nickname", "email", "local"] {
                assert_eq!(
                    items
                        .iter()
                        .find(|item| item.label == name)
                        .unwrap()
                        .sort_text
                        .as_deref(),
                    Some(format!("0_{name}").as_str())
                );
            }
        }
    }
    let items = complete_statement(&backend, &uri, "$user->§").await;
    assert!(items.iter().any(|item| {
        item.label == "nickname"
            && item
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("string"))
    }));
}

#[tokio::test]
async fn model_column_attributes_and_properties_inherit_independently() {
    let account = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Attributes\{Fillable, Hidden, Visible};
#[Fillable(['parent_attribute'])]
#[Hidden(['parent_secret'])]
#[Visible(['parent_visible'])]
class Account extends \Illuminate\Database\Eloquent\Model {
    protected $fillable = ['parent_property'];
    protected $hidden = ['property_secret'];
}
"#;
    let user = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Attributes\{Fillable, Visible};
#[Fillable(['child_attribute'])]
#[Visible([])]
class User extends Account { protected $hidden = []; }
"#;
    let (backend, _dir, uri) = workspace_with_models(account, user);
    let items = complete_statement(&backend, &uri, "User::create(['§']);").await;
    for name in ["parent_property", "child_attribute", "parent_secret"] {
        assert!(
            items.iter().any(|item| item.label == name),
            "missing {name}: {items:?}"
        );
    }
    for name in ["parent_attribute", "parent_visible", "property_secret"] {
        assert!(
            !items.iter().any(|item| item.label == name),
            "unexpected {name}: {items:?}"
        );
    }
    for name in ["parent_property", "child_attribute"] {
        assert_eq!(
            items
                .iter()
                .find(|item| item.label == name)
                .unwrap()
                .sort_text
                .as_deref(),
            Some(format!("0_{name}").as_str())
        );
    }
}

#[tokio::test]
async fn model_column_attributes_respect_guarded_property_and_unguarded_precedence() {
    for (property, attributes, expect_attribute) in [
        ("", "#[Guarded(['attribute_secret'])]", true),
        (
            "protected $guarded = ['*'];",
            "#[Guarded(['attribute_secret'])]",
            true,
        ),
        (
            "protected $guarded = [];",
            "#[Guarded(['attribute_secret'])]",
            false,
        ),
        (
            "protected $guarded = ['property_secret'];",
            "#[Guarded(['attribute_secret'])]",
            false,
        ),
        ("", "#[Guarded(['attribute_secret']), Unguarded]", false),
        (
            "protected $guarded = ['property_secret'];",
            "#[Guarded(['attribute_secret']), Unguarded]",
            false,
        ),
    ] {
        let account = format!(
            "<?php namespace App\\Models; class Account extends \\Illuminate\\Database\\Eloquent\\Model {{ {property} }}"
        );
        let user = format!(
            "<?php namespace App\\Models; use Illuminate\\Database\\Eloquent\\Attributes\\{{Guarded, Unguarded}}; {attributes} class User extends Account {{}}"
        );
        let (backend, _dir, uri) = workspace_with_models(&account, &user);
        let items = complete_statement(&backend, &uri, "User::where('§', 1);").await;
        assert_eq!(
            items.iter().any(|item| item.label == "attribute_secret"),
            expect_attribute,
            "{property} {attributes}: {items:?}"
        );
        assert_eq!(
            items.iter().any(|item| item.label == "property_secret"),
            property.contains("property_secret")
        );
        assert!(!items.iter().any(|item| item.label == "*"));
    }
}

#[tokio::test]
async fn model_column_attributes_from_direct_traits_follow_class_precedence() {
    let account = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Attributes\{Fillable, Hidden};
#[Fillable(['ancestor'])]
class Account extends \Illuminate\Database\Eloquent\Model {}
#[Fillable(['trait_fillable']), Hidden(['trait_hidden'])]
trait Columns {}
#[Hidden(['nested_hidden'])]
trait NestedColumns {}
trait Wrapper { use NestedColumns; }
"#;
    let user = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Attributes\Hidden;
#[Hidden(['own_hidden'])]
class User extends Account { use Columns, Wrapper; }
"#;
    let (backend, _dir, uri) = workspace_with_models(account, user);
    let items = complete_statement(&backend, &uri, "User::create(['§']);").await;
    for name in ["trait_fillable", "own_hidden"] {
        assert!(items.iter().any(|item| item.label == name), "{items:?}");
    }
    for name in ["ancestor", "trait_hidden", "nested_hidden"] {
        assert!(!items.iter().any(|item| item.label == name), "{items:?}");
    }
}

#[tokio::test]
async fn model_column_attributes_update_after_provider_file_edits() {
    let source = "<?php namespace App\\Models; #[\\Illuminate\\Database\\Eloquent\\Attributes\\Fillable(['before'])] class User extends Account {}";
    let (backend, dir, uri) = workspace_with_models(ACCOUNT, source);
    let items = complete_statement(&backend, &uri, "User::create(['§']);").await;
    assert!(items.iter().any(|item| item.label == "before"));
    let model_uri = Uri::from_file_path(dir.path().join("app/Models/User.php")).unwrap();
    backend.update_ast(model_uri.as_str(), &source.replace("before", "after"));
    let items = complete_statement(&backend, &uri, "User::create(['§']);").await;
    assert!(items.iter().any(|item| item.label == "after"));
    assert!(!items.iter().any(|item| item.label == "before"));
}

#[tokio::test]
async fn model_column_attributes_index_imports_and_namespaces_before_extraction() {
    for source in [
        r#"<?php
use Illuminate\Database\Eloquent\Attributes\Fillable as Fields;
#[Fields(['nickname'])]
class User extends \App\Models\Account {}
(new User())->§
"#,
        r#"<?php
namespace App;
use Illuminate\Database\Eloquent\Attributes as Columns;
#[Columns\Fillable(['nickname'])]
class User extends \App\Models\Account {}
(new User())->§
"#,
        r#"<?php
namespace Other { use Other\Fillable as Fields; #[Fields(['wrong'])] class Other {} }
namespace App {
use Illuminate\Database\Eloquent\Attributes\Fillable as Fields;
#[Fields(['nickname'])]
class User extends \App\Models\Account {}
(new User())->§
}
"#,
    ] {
        let (backend, _dir, uri) = workspace();
        let (source, position) = split_cursor(source);
        let items = complete_at(&backend, &uri, &source, position.line, position.character).await;
        assert!(
            items.iter().any(|item| item.label == "nickname"),
            "{source}: {items:?}"
        );
        assert!(!items.iter().any(|item| item.label == "wrong"));
    }
}

#[tokio::test]
async fn model_column_attributes_use_pivots_empty_guarded_default() {
    for (property, expected) in [("", true), ("protected $guarded = ['*'];", false)] {
        let user = format!(
            r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Attributes\Guarded;
#[Guarded(['secret'])]
class User extends \Illuminate\Database\Eloquent\Relations\Pivot {{ {property} }}
"#
        );
        let (backend, _dir, uri) = workspace_with_models(ACCOUNT, &user);
        let items = complete_statement(&backend, &uri, "User::where('§', 1);").await;
        assert_eq!(
            items.iter().any(|item| item.label == "secret"),
            expected,
            "{property}: {items:?}"
        );
    }
}
