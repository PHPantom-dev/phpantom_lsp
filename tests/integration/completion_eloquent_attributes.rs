//! Column completions in Eloquent attribute arrays and scalar arguments.

use crate::common::{
    complete_at, create_psr4_workspace, item_labels, position_after, split_cursor,
};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionTextEdit, Range, Url};

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

fn workspace() -> (Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("framework/Model.php", MODEL),
            ("framework/Builder.php", BUILDER),
            ("framework/Relations/HasMany.php", HAS_MANY),
            ("app/Models/Account.php", ACCOUNT),
            ("app/Models/User.php", USER),
            ("app/Models/Post.php", POST),
        ],
    );
    let uri = Url::from_file_path(dir.path().join("app/Consumer.php")).unwrap();
    (backend, dir, uri)
}

async fn complete_statement(backend: &Backend, uri: &Url, statement: &str) -> Vec<CompletionItem> {
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
        assert_eq!(item.kind, Some(CompletionItemKind::FIELD));
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
    let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
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
