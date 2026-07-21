use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Shared stubs ───────────────────────────────────────────────────────────

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\Models\\": "src/Models/",
            "Illuminate\\Database\\Eloquent\\": "vendor/illuminate/Eloquent/",
            "Illuminate\\Database\\Eloquent\\Relations\\": "vendor/illuminate/Eloquent/Relations/",
            "Illuminate\\Database\\Concerns\\": "vendor/illuminate/Concerns/",
            "Illuminate\\Database\\Query\\": "vendor/illuminate/Query/"
        }
    }
}"#;

/// Eloquent Model stub matching real Laravel: no `@mixin`, just a
/// `query()` method returning `Builder<static>`.  The LSP's
/// `find_builder_forwarded_method` handles the __callStatic delegation
/// internally.
const MODEL_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Eloquent;
abstract class Model {
    const CREATED_AT = 'created_at';
    const UPDATED_AT = 'updated_at';
    /** @return \\Illuminate\\Database\\Eloquent\\Builder<static> */
    public static function query() {}
    /** @return \\Illuminate\\Database\\Eloquent\\Builder<static> */
    public static function with(mixed $relations) {}
}
";

const BUILDER_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Eloquent;

/**
 * @template TModel of \\Illuminate\\Database\\Eloquent\\Model
 * @mixin \\Illuminate\\Database\\Query\\Builder
 */
class Builder {
    /** @use \\Illuminate\\Database\\Concerns\\BuildsQueries<TModel> */
    use \\Illuminate\\Database\\Concerns\\BuildsQueries;

    /**
     * @param  string|array  $column
     * @return $this
     */
    public function where($column, $operator = null, $value = null, $boolean = 'and') {}
    /** @return \\Illuminate\\Database\\Eloquent\\Collection<int, TModel> */
    public function get($columns = null) { return new Collection(); }
    /** @return \\Illuminate\\Support\\Collection<array-key, mixed> */
    public function pluck($column, $key = null) {}
}
";

const BUILDS_QUERIES_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Concerns;

/**
 * @template TValue
 */
trait BuildsQueries {
    /** @return TValue|null */
    public function first($columns = null) { return null; }
}
";

const QUERY_BUILDER_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Query;
class Builder {
    /**
     * @return $this
     */
    public function whereIn($column, $values, $boolean = 'and', $not = false) { return $this; }
    /**
     * @return $this
     */
    public function groupBy(...$groups) { return $this; }
    /**
     * @return $this
     */
    public function orderBy($column, $direction = 'asc') { return $this; }
    /**
     * @return $this
     */
    public function limit($value) { return $this; }
}
";

const COLLECTION_PHP: &str = "\
<?php
namespace Illuminate\\Database\\Eloquent;
/**
 * @template TKey of array-key
 * @template TModel
 */
class Collection {
    /** @return TModel|null */
    public function first(): mixed { return null; }
    public function count(): int { return 0; }
}
";

/// Standard set of framework stub files.
fn framework_stubs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vendor/illuminate/Eloquent/Model.php", MODEL_PHP),
        ("vendor/illuminate/Eloquent/Builder.php", BUILDER_PHP),
        ("vendor/illuminate/Eloquent/Collection.php", COLLECTION_PHP),
        (
            "vendor/illuminate/Concerns/BuildsQueries.php",
            BUILDS_QUERIES_PHP,
        ),
        ("vendor/illuminate/Query/Builder.php", QUERY_BUILDER_PHP),
    ]
}

/// Build a PSR-4 workspace from the framework stubs plus extra app files.
fn make_workspace(app_files: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let mut files: Vec<(&str, &str)> = framework_stubs();
    files.extend_from_slice(app_files);
    create_psr4_workspace(COMPOSER_JSON, &files)
}

/// Helper: open a file and trigger go-to-definition, returning the location.
async fn goto_definition_at(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    relative_path: &str,
    content: &str,
    line: u32,
    character: u32,
) -> Option<GotoDefinitionResponse> {
    let uri = Url::from_file_path(dir.path().join(relative_path)).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    backend.goto_definition(params).await.unwrap()
}

/// Extract the target line number from a definition response.
fn definition_line(response: &GotoDefinitionResponse) -> u32 {
    match response {
        GotoDefinitionResponse::Scalar(location) => location.range.start.line,
        GotoDefinitionResponse::Array(locations) => locations[0].range.start.line,
        GotoDefinitionResponse::Link(links) => links[0].target_range.start.line,
    }
}

/// Extract the target URI from a definition response.
fn definition_uri(response: &GotoDefinitionResponse) -> &Url {
    match response {
        GotoDefinitionResponse::Scalar(location) => &location.uri,
        GotoDefinitionResponse::Array(locations) => &locations[0].uri,
        GotoDefinitionResponse::Link(links) => &links[0].target_uri,
    }
}

// ─── Builder-forwarded static method go-to-definition ───────────────────────

#[tokio::test]
async fn test_goto_definition_builder_forwarded_where_on_model() {
    // BlogAuthor::where() should jump to Builder::where().
    // The real Model has no @mixin; the definition resolver's
    // find_builder_forwarded_method bridges the gap.
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BlogAuthor extends Model {
    public function demo(): void {
        BlogAuthor::where('active', true);
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // Cursor on "where" in `BlogAuthor::where('active', true);`
    // Line 5 (0-indexed), "where" starts at character 20
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        5,
        22,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on BlogAuthor::where() should resolve to Builder::where()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    let uri_str = uri.as_str();
    assert!(
        uri_str.contains("Builder.php"),
        "Should jump to Builder.php, got: {}",
        uri_str
    );
}

#[tokio::test]
async fn test_goto_definition_builder_where_on_model_with_scopes() {
    // BlogAuthor::where() should jump to Builder::where() even when
    // the model has scope methods defined.  Scope methods (scopeActive,
    // scopeOfGenre) must not interfere with the mixin-based resolution
    // of Builder methods like `where`.
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class BlogAuthor extends Model {
    public function scopeActive(Builder $query): void {
        $query->where('active', true);
    }
    public function scopeOfGenre(Builder $query, string $genre): void {
        $query->where('genre', $genre);
    }
    public function demo(): void {
        BlogAuthor::where('active', true);
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // Cursor on "where" in `BlogAuthor::where('active', true);`
    // Line 12 (0-indexed), "where" starts at character 20
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        12,
        22,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on BlogAuthor::where() (with scope methods present) should resolve to Builder::where()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Builder.php"),
        "Should jump to Builder.php, got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_builder_forwarded_orderby_on_model() {
    // orderBy lives on Query\Builder, reached via Eloquent\Builder's
    // @mixin.  The definition resolver finds it through
    // find_builder_forwarded_method → find_declaring_class(builder).
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BlogAuthor extends Model {
    public function demo(): void {
        BlogAuthor::orderBy('name');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // Cursor on "orderBy" in `BlogAuthor::orderBy('name');`
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        5,
        22,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on BlogAuthor::orderBy() should resolve to Query\\Builder::orderBy()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Builder.php"),
        "Should jump to a Builder.php file, got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_scope_method_on_model() {
    // Scope methods are defined on the model itself as scopeXxx.
    // go-to-definition on `BlogAuthor::active()` should jump to
    // the `scopeActive` method in BlogAuthor.
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class BlogAuthor extends Model {
    public function scopeActive(Builder $query): void {
        $query->where('active', true);
    }
    public function demo(): void {
        BlogAuthor::active();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // Cursor on "active" in `BlogAuthor::active();`
    // Line 9 (0-indexed), "active" starts at character 20
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        9,
        22,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on BlogAuthor::active() should resolve to scopeActive"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("BlogAuthor.php"),
        "Scope should resolve within BlogAuthor.php, got: {}",
        uri.as_str()
    );

    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeActive is on line 5 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_query_builder_mixin_method_on_model() {
    // Query\Builder methods (via @mixin on Eloquent\Builder) are
    // reached through find_builder_forwarded_method → find_declaring_class
    // which walks Builder's @mixin chain.
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BlogAuthor extends Model {
    public function demo(): void {
        BlogAuthor::whereIn('id', [1, 2, 3]);
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // Cursor on "whereIn" in `BlogAuthor::whereIn('id', [1, 2, 3]);`
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        5,
        22,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on BlogAuthor::whereIn() should resolve through Builder's @mixin to Query\\Builder::whereIn()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    // whereIn is on Query\Builder, which Eloquent\Builder mixes in.
    assert!(
        uri.as_str().contains("Builder.php"),
        "Should jump to a Builder.php file, got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_chained_builder_method() {
    // go-to-definition on orderBy when $q is typed as Builder directly.
    // This isolates find_declaring_class from variable resolution.
    // orderBy is on Query\Builder, which Eloquent\Builder has via @mixin.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class User extends Model {
    /** @param Builder $q */
    public function demo(Builder $q): void {
        $q->orderBy('name');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "orderBy" in `$q->orderBy('name');`
    // Line 7 (0-indexed), "orderBy" starts at character 12
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 7, 14).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->orderBy() (where $q is Builder) should resolve to Query\\Builder::orderBy()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Builder.php"),
        "Should jump to Builder.php, got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_chained_builder_method_via_variable() {
    // go-to-definition on orderBy when $q is assigned from User::where().
    // This tests both variable resolution and find_declaring_class.
    //
    // Uses a method with a native return type hint (`: Builder`) on the
    // helper so that variable resolution doesn't depend on virtual member
    // resolution working inside the variable-resolution parse pass.
    // orderBy is on Query\Builder, reached via Builder's @mixin.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class User extends Model {
    /** @return Builder */
    public static function myWhere(): Builder { return new Builder(); }
    public function test() {
        $q = User::myWhere();
        $q->orderBy('name');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "orderBy" in `$q->orderBy('name');`
    // Line 9 (0-indexed), "orderBy" starts at character 12
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 9, 14).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->orderBy() (where $q is Builder via myWhere()) should resolve to Query\\Builder::orderBy()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Builder.php"),
        "Should jump to Builder.php, got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_builder_forwarded_via_variable_assignment() {
    // go-to-definition on orderBy when $q is assigned from User::where()
    // (the actual builder-forwarded virtual method). This relies on
    // variable resolution resolving the virtual static method's return type.
    // orderBy is on Query\Builder, reached via Eloquent\Builder's @mixin.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    public function test() {
        $q = User::where('active', true);
        $q->orderBy('name');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "orderBy" in `$q->orderBy('name');`
    // Line 6 (0-indexed), "orderBy" starts at character 12
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 6, 14).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->orderBy() (where $q = User::where()) should resolve"
    );
    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Query/Builder.php"),
        "Should jump to Query/Builder.php (where orderBy is declared), got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_builder_forwarded_via_variable_get() {
    // go-to-definition on get() when $q is assigned from User::where().
    // get() is on Eloquent\Builder.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    public function test() {
        $q = User::where('active', true);
        $q->get();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "get" in `$q->get();`
    // Line 6 (0-indexed), "get" starts at character 12
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 6, 13).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->get() (where $q = User::where()) should resolve"
    );
    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Eloquent/Builder.php"),
        "Should jump to Eloquent/Builder.php (where get is declared), got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_builder_forwarded_via_variable_first() {
    // go-to-definition on first() when $q is assigned from User::where().
    // first() is on the BuildsQueries trait.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    public function test() {
        $q = User::where('active', true);
        $q->first();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "first" in `$q->first();`
    // Line 6 (0-indexed), "first" starts at character 12
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 6, 13).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->first() (where $q = User::where()) should resolve"
    );
    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("BuildsQueries.php"),
        "Should jump to BuildsQueries.php (where first is declared), got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_builder_forwarded_via_variable_chained_assignment() {
    // go-to-definition on get() when $q is assigned from a chained
    // builder call: User::where(...)->orderBy(...).
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    public function test() {
        $q = User::where('active', true)->orderBy('name');
        $q->get();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "get" in `$q->get();`
    // Line 6 (0-indexed), "get" starts at character 12
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 6, 13).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->get() (where $q = User::where()->orderBy()) should resolve"
    );
    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Eloquent/Builder.php"),
        "Should jump to Eloquent/Builder.php (where get is declared), got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_builder_method_on_indirect_model() {
    // A model that extends another model (which extends Eloquent\Model)
    // should also resolve builder-forwarded methods via find_builder_forwarded_method.
    let base_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BaseModel extends Model {}
";
    let child_php = "\
<?php
namespace App\\Models;
class ChildModel extends BaseModel {
    public function demo(): void {
        ChildModel::where('id', 1);
    }
}
";
    let (backend, dir) = make_workspace(&[
        ("src/Models/BaseModel.php", base_php),
        ("src/Models/ChildModel.php", child_php),
    ]);

    // Cursor on "where" in `ChildModel::where('id', 1);`
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/ChildModel.php",
        child_php,
        4,
        22,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on ChildModel::where() should resolve to Builder::where()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Builder.php"),
        "Should jump to Builder.php, got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_model_own_method_preferred_over_builder() {
    // If the model defines its own `where` method, go-to-definition should
    // jump to the model's own method, not the Builder's.  The normal
    // find_declaring_class finds it before the builder fallback fires.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    public static function where(string $col, mixed $val = null): static {
        return new static();
    }
    public function demo(): void {
        User::where('active', true);
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "where" in `User::where('active', true);`
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 8, 16).await;

    assert!(
        result.is_some(),
        "Go-to-definition on User::where() should resolve to User's own where()"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("User.php"),
        "Should jump to User.php (own method), got: {}",
        uri.as_str()
    );

    let line = definition_line(&response);
    assert_eq!(line, 4, "User's own where() is on line 4, got: {}", line);
}

// ─── Go-to-definition for Eloquent virtual properties ───────────────────────

#[tokio::test]
async fn test_goto_definition_legacy_accessor_property() {
    // Ctrl+click on `$author->display_name` should jump to
    // `getDisplayNameAttribute()` method.
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BlogAuthor extends Model {
    public function getDisplayNameAttribute(): string {
        return 'display';
    }
    public function demo(): void {
        $author = new BlogAuthor();
        $author->display_name;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // "display_name" on line 9, cursor at character 18
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        9,
        18,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on $author->display_name should resolve"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 4,
        "Should jump to getDisplayNameAttribute on line 4, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_modern_accessor_property() {
    // Ctrl+click on `$author->avatar_url` should jump to
    // `avatarUrl()` method.
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BlogAuthor extends Model {
    protected function avatarUrl(): \\Illuminate\\Database\\Eloquent\\Casts\\Attribute {
        return new \\Illuminate\\Database\\Eloquent\\Casts\\Attribute();
    }
    public function demo(): void {
        $author = new BlogAuthor();
        $author->avatar_url;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // "avatar_url" on line 9, cursor at character 18
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        9,
        18,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on $author->avatar_url should resolve"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 4,
        "Should jump to avatarUrl() on line 4, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_snake_case_does_not_jump_to_relationship_method() {
    // Ctrl+click on `$bakery->master_recipe` should NOT jump to the
    // `masterRecipe()` relationship method.  The relationship property
    // name is `masterRecipe` (no snake_case conversion), so
    // `master_recipe` is not a real property.  Previously the accessor
    // fallback in GTD ran snake_to_camel, found `masterRecipe()`, and
    // jumped to it even though it is not an accessor.
    let bakery_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Bakery extends Model {
    /** @return \\Illuminate\\Database\\Eloquent\\Relations\\BelongsToMany<Recipe, $this> */
    public function masterRecipe(): mixed {
        return $this->belongsToMany(Recipe::class);
    }
    public function demo(): void {
        $bakery = new Bakery();
        $bakery->master_recipe;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Bakery.php", bakery_php)]);

    // "master_recipe" on line 10, cursor at character 18
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Bakery.php", bakery_php, 10, 18).await;

    // Should NOT resolve — master_recipe is not a real property.
    // If it resolves, it means the accessor fallback incorrectly matched
    // the relationship method.
    if let Some(response) = result {
        let line = definition_line(&response);
        assert_ne!(
            line, 5,
            "Should NOT jump to masterRecipe() relationship method (line 5) for snake_case master_recipe"
        );
    }
}

#[tokio::test]
async fn test_goto_definition_casts_property_entry() {
    // Ctrl+click on `$user->is_admin` should jump to the 'is_admin'
    // entry in the $casts array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $casts = [
        'is_admin' => 'boolean',
        'created_at' => 'datetime',
    ];
    public function demo(): void {
        $user = new User();
        $user->is_admin;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "is_admin" on line 10, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 10, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->is_admin should resolve to $casts entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'is_admin' in $casts on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_casts_method_entry() {
    // Ctrl+click on `$user->verified_at` should jump to the
    // 'verified_at' entry in the casts() method return array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected function casts(): array {
        return [
            'verified_at' => 'datetime',
        ];
    }
    public function demo(): void {
        $user = new User();
        $user->verified_at;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "verified_at" on line 11, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 11, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->verified_at should resolve to casts() entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 6,
        "Should jump to 'verified_at' in casts() on line 6, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_attributes_default_entry() {
    // Ctrl+click on `$user->role` should jump to the 'role' entry
    // in the $attributes array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $attributes = [
        'role' => 'user',
        'is_active' => true,
    ];
    public function demo(): void {
        $user = new User();
        $user->role;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "role" on line 10, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 10, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->role should resolve to $attributes entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'role' in $attributes on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_fillable_column_name() {
    // Ctrl+click on `$user->name` should jump to the 'name' entry
    // in the $fillable array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $fillable = [
        'name',
        'email',
    ];
    public function demo(): void {
        $user = new User();
        $user->name;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "name" on line 10, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 10, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->name should resolve to $fillable entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'name' in $fillable on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_dates_property_entry() {
    // Ctrl+click on `$user->deleted_at` should jump to the 'deleted_at'
    // entry in the $dates array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $dates = [
        'deleted_at',
        'trial_ends_at',
    ];
    public function demo(): void {
        $user = new User();
        $user->deleted_at;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "deleted_at" on line 10, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 10, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->deleted_at should resolve to $dates entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'deleted_at' in $dates on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_where_property_from_fillable() {
    // Ctrl+click on `Bakery::whereFlour(...)` should jump to the 'flour'
    // entry in the $fillable array.
    let model_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Bakery extends Model {
    protected $fillable = [
        'flour',
        'water',
    ];
    public function demo(): void {
        Bakery::whereFlour('rye');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Bakery.php", model_php)]);

    // "whereFlour" on line 9, cursor at character 17
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Bakery.php", model_php, 9, 17).await;

    assert!(
        result.is_some(),
        "Go-to-definition on Bakery::whereFlour should resolve to $fillable entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'flour' in $fillable on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_where_property_from_casts() {
    // Ctrl+click on `Bakery::whereApricot(...)` should jump to the 'apricot'
    // entry in the $casts array.
    let model_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Bakery extends Model {
    protected $casts = [
        'apricot' => 'boolean',
    ];
    public function demo(): void {
        Bakery::whereApricot(true);
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Bakery.php", model_php)]);

    // "whereApricot" on line 8, cursor at character 17
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Bakery.php", model_php, 8, 17).await;

    assert!(
        result.is_some(),
        "Go-to-definition on Bakery::whereApricot should resolve to $casts entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'apricot' in $casts on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_where_property_from_dates() {
    // Ctrl+click on `Bakery::whereDefrostedAt(...)` should jump to the
    // 'defrosted_at' entry in the $dates array.
    let model_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Bakery extends Model {
    protected $dates = [
        'defrosted_at',
    ];
    public function demo(): void {
        Bakery::whereDefrostedAt('2024-01-01');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Bakery.php", model_php)]);

    // "whereDefrostedAt" on line 8, cursor at character 17
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Bakery.php", model_php, 8, 17).await;

    assert!(
        result.is_some(),
        "Go-to-definition on Bakery::whereDefrostedAt should resolve to $dates entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'defrosted_at' in $dates on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_hidden_column_name() {
    // Ctrl+click on `$user->password` should jump to the 'password'
    // entry in the $hidden array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $hidden = [
        'password',
        'remember_token',
    ];
    public function demo(): void {
        $user = new User();
        $user->password;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "password" on line 10, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 10, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->password should resolve to $hidden entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'password' in $hidden on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_guarded_column_name() {
    // Ctrl+click on `$user->secret_key` should jump to the
    // 'secret_key' entry in the $guarded array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $guarded = [
        'secret_key',
    ];
    public function demo(): void {
        $user = new User();
        $user->secret_key;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "secret_key" on line 9, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 9, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->secret_key should resolve to $guarded entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'secret_key' in $guarded on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_visible_column_name() {
    // Ctrl+click on `$user->website` should jump to the 'website'
    // entry in the $visible array.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $visible = [
        'website',
        'avatar',
    ];
    public function demo(): void {
        $user = new User();
        $user->website;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "website" on line 10, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 10, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->website should resolve to $visible entry"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to 'website' in $visible on line 5, got: {}",
        line
    );
}

// ─── Builder method GTD on chained Builder instances ────────────────────────

#[tokio::test]
async fn test_goto_definition_builder_method_on_chained_builder_instance() {
    // BrandTranslation::where('name', 1)->pluck() — GTD on pluck should
    // jump to pluck() on the Eloquent Builder class.
    let model_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BrandTranslation extends Model {}
";
    let (backend, dir) = make_workspace(&[("src/Models/BrandTranslation.php", model_php)]);

    // Open model file first so it's indexed
    let model_uri =
        Url::from_file_path(dir.path().join("src/Models/BrandTranslation.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: model_uri,
                language_id: "php".to_string(),
                version: 1,
                text: model_php.to_string(),
            },
        })
        .await;

    let test_php = "\
<?php
namespace App\\Models;
class TestService {
    public function demo(): void {
        BrandTranslation::where('name', 1)->pluck('brand_id');
    }
}
";
    // Cursor on "pluck" in `->pluck('brand_id')`
    // Line 4 (0-indexed), "pluck" starts at character 44
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/TestService.php",
        test_php,
        4,
        46,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on ->pluck() after Model::where() should resolve"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Builder.php"),
        "pluck should resolve to Builder.php, got: {}",
        uri.as_str()
    );
    // pluck is on the Eloquent Builder — verify it lands on the right file
    assert!(
        uri.as_str().contains("Eloquent"),
        "pluck should resolve to the Eloquent Builder, got: {}",
        uri.as_str()
    );
}

#[tokio::test]
async fn test_goto_definition_builder_method_via_variable_on_chained_builder() {
    // $q = BrandTranslation::where('name', 1); $q->pluck() — GTD on pluck
    let model_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class BrandTranslation extends Model {}
";
    let (backend, dir) = make_workspace(&[("src/Models/BrandTranslation.php", model_php)]);

    let model_uri =
        Url::from_file_path(dir.path().join("src/Models/BrandTranslation.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: model_uri,
                language_id: "php".to_string(),
                version: 1,
                text: model_php.to_string(),
            },
        })
        .await;

    let test_php = "\
<?php
namespace App\\Models;
class TestService {
    public function demo(): void {
        $q = BrandTranslation::where('name', 1);
        $q->pluck('brand_id');
    }
}
";
    // Cursor on "pluck" in `$q->pluck('brand_id');`
    // Line 5 (0-indexed), "pluck" starts at character 12
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/TestService.php",
        test_php,
        5,
        14,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->pluck() should resolve"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Builder.php"),
        "pluck should resolve to Builder.php, got: {}",
        uri.as_str()
    );
    assert!(
        uri.as_str().contains("Eloquent"),
        "pluck should resolve to the Eloquent Builder, got: {}",
        uri.as_str()
    );
}

// ─── Scope methods on Builder instances (GTD) ───────────────────────────────

#[tokio::test]
async fn test_goto_definition_scope_on_builder_after_where_chain() {
    // Brand::where('id', 1)->isActive() — GTD on isActive should jump
    // to scopeIsActive on the model.
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeIsActive(Builder $query): void {
        $query->where('active', true);
    }
    public function demo(): void {
        $q = Brand::where('id', 1);
        $q->isActive();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "isActive" in `$q->isActive();`
    // Line 10 (0-indexed), "isActive" starts at character 12
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 10, 14).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->isActive() should resolve to scopeIsActive"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Brand.php"),
        "Scope should resolve within Brand.php, got: {}",
        uri.as_str()
    );

    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeIsActive is on line 5 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_scope_on_builder_inline_chain() {
    // Brand::where('id', 1)->isActive() — inline chain, GTD on isActive
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeIsActive(Builder $query): void {
        $query->where('active', true);
    }
    public function demo(): void {
        Brand::where('id', 1)->isActive();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "isActive" in `Brand::where('id', 1)->isActive();`
    // Line 9 (0-indexed), "isActive" starts at character 31
    let result = goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 9, 33).await;

    assert!(
        result.is_some(),
        "Go-to-definition on Brand::where()->isActive() should resolve to scopeIsActive"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeIsActive is on line 5 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_scope_on_builder_with_params() {
    // $q->ofGenre('fiction') — GTD on ofGenre should jump to scopeOfGenre
    let author_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class BlogAuthor extends Model {
    public function scopeOfGenre(Builder $query, string $genre): void {
        $query->where('genre', $genre);
    }
    public function demo(): void {
        $q = BlogAuthor::where('active', true);
        $q->ofGenre('fiction');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/BlogAuthor.php", author_php)]);

    // Cursor on "ofGenre" in `$q->ofGenre('fiction');`
    // Line 10 (0-indexed), "ofGenre" starts at character 12
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Models/BlogAuthor.php",
        author_php,
        10,
        14,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on $q->ofGenre() should resolve to scopeOfGenre"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("BlogAuthor.php"),
        "Scope should resolve within BlogAuthor.php, got: {}",
        uri.as_str()
    );

    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeOfGenre is on line 5 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_scope_inside_scope_body() {
    // Inside scopeActive body, $query->verified() — GTD on verified
    // should jump to scopeVerified.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class User extends Model {
    public function scopeActive(Builder $query): void {
        $query->verified();
    }
    public function scopeVerified(Builder $query): void {
        $query->where('verified', true);
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // Cursor on "verified" in `$query->verified();`
    // Line 6 (0-indexed), "verified" starts at character 16
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 6, 18).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $query->verified() inside scope body should resolve to scopeVerified"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 8,
        "scopeVerified is on line 8 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_scope_on_builder_after_scope_chain() {
    // Brand::where('id', 1)->isActive()->ofType('premium') — GTD on
    // ofType after chaining through another scope.
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeIsActive(Builder $query): void {}
    public function scopeOfType(Builder $query, string $type): void {}
    public function demo(): void {
        Brand::where('id', 1)->isActive()->ofType('premium');
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "ofType" in `->ofType('premium');`
    // Line 8 (0-indexed), "ofType" starts at character 42
    let result = goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 8, 44).await;

    assert!(
        result.is_some(),
        "Go-to-definition on ->ofType() after scope chain should resolve to scopeOfType"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 6,
        "scopeOfType is on line 6 (0-indexed), got: {}",
        line
    );
}

// ─── GTD for scope methods called through with() ────────────────────────────

#[tokio::test]
async fn test_goto_definition_scope_on_builder_after_with() {
    // Brand::with('english')->sortable() — GTD on sortable should jump
    // to scopeSortable on the model.  `with()` returns Builder<static>,
    // so the chain resolves to Builder<Brand> which has scope methods
    // injected.
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeSortable(Builder $query): void {
        $query->orderBy('name');
    }
    public function demo(): void {
        Brand::with('english')->sortable();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "sortable" in `Brand::with('english')->sortable();`
    // Line 9 (0-indexed): "        Brand::with('english')->sortable();"
    // "sortable" starts at character 32
    let result = goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 9, 34).await;

    assert!(
        result.is_some(),
        "Go-to-definition on ->sortable() after with() should resolve to scopeSortable"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Brand.php"),
        "Scope should resolve within Brand.php, got: {}",
        uri.as_str()
    );

    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeSortable is on line 5 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_scope_on_builder_after_with_then_where() {
    // Brand::with('english')->where('active', 1)->sortable() — GTD on
    // sortable should still resolve through the chained Builder<Brand>.
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeSortable(Builder $query): void {
        $query->orderBy('name');
    }
    public function demo(): void {
        Brand::with('english')->where('active', 1)->sortable();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "sortable" in `->sortable();`
    // Line 9 (0-indexed): "        Brand::with('english')->where('active', 1)->sortable();"
    // "sortable" starts at character 52
    let result = goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 9, 54).await;

    assert!(
        result.is_some(),
        "Go-to-definition on ->sortable() after with()->where() should resolve to scopeSortable"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeSortable is on line 5 (0-indexed), got: {}",
        line
    );
}

// ─── GTD with blank lines in method chains ──────────────────────────────────

#[tokio::test]
async fn test_goto_definition_with_blank_line_in_chain() {
    // A blank line between chain segments should not break GTD.
    //
    //   Brand::where('id', 1)
    //
    //       ->isActive()   // GTD on isActive should still work
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeIsActive(Builder $query): void {
        $query->where('active', true);
    }
    public function demo(): void {
        Brand::where('id', 1)

            ->isActive();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "isActive" in `            ->isActive();`
    // Line 11 (0-indexed), "isActive" starts at character 14
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 11, 16).await;

    assert!(
        result.is_some(),
        "GTD on ->isActive() should work even with a blank line in the chain"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Brand.php"),
        "Scope should resolve within Brand.php, got: {}",
        uri.as_str()
    );

    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeIsActive is on line 5 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_builder_method_with_blank_line_in_chain() {
    // Also verify that regular (non-scope) builder methods work across
    // blank lines in the chain.
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function demo(): void {
        Brand::where('id', 1)

            ->get();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "get" in `            ->get();`
    // Line 8 (0-indexed), "get" starts at character 14
    let result = goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 8, 15).await;

    assert!(
        result.is_some(),
        "GTD on ->get() should work even with a blank line in the chain"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    // get() is defined on Builder — just verify we got a result (line varies
    // depending on the stub).
    assert!(
        line < 100,
        "get() definition line should be reasonable, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_scope_on_builder_forwarded_with() {
    // When `with()` is NOT defined on Model but only on Builder (as in
    // real Laravel), it reaches Model via __callStatic forwarding.
    // GTD on a scope method after `Brand::with('english')->sortable()`
    // should still resolve to `scopeSortable` on the model.

    // Use a Model stub WITHOUT `with()` — the builder-forwarding logic
    // in the LSP will delegate the static call to Builder.
    let model_no_with = "\
<?php
namespace Illuminate\\Database\\Eloquent;
abstract class Model {
    /** @return \\Illuminate\\Database\\Eloquent\\Builder<static> */
    public static function query() {}
}
";

    // Builder stub that declares `with()` as an instance method returning `$this`.
    let builder_with = "\
<?php
namespace Illuminate\\Database\\Eloquent;

/**
 * @template TModel of \\Illuminate\\Database\\Eloquent\\Model
 * @mixin \\Illuminate\\Database\\Query\\Builder
 */
class Builder {
    /** @use \\Illuminate\\Database\\Concerns\\BuildsQueries<TModel> */
    use \\Illuminate\\Database\\Concerns\\BuildsQueries;

    /** @return $this */
    public function where($column, $operator = null, $value = null, $boolean = 'and') {}
    /** @return $this */
    public function with(mixed $relations) {}
    /** @return \\Illuminate\\Database\\Eloquent\\Collection<int, TModel> */
    public function get($columns = null) { return new Collection(); }
}
";

    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    public function scopeSortable(Builder $query): void {
        $query->orderBy('name');
    }
    public function demo(): void {
        Brand::with('english')->sortable();
    }
}
";

    let files: Vec<(&str, &str)> = vec![
        ("vendor/illuminate/Eloquent/Model.php", model_no_with),
        ("vendor/illuminate/Eloquent/Builder.php", builder_with),
        ("vendor/illuminate/Eloquent/Collection.php", COLLECTION_PHP),
        (
            "vendor/illuminate/Concerns/BuildsQueries.php",
            BUILDS_QUERIES_PHP,
        ),
        ("vendor/illuminate/Query/Builder.php", QUERY_BUILDER_PHP),
        ("src/Models/Brand.php", brand_php),
    ];
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);

    // Cursor on "sortable" in `Brand::with('english')->sortable();`
    // Line 9 (0-indexed), "sortable" starts at character 32
    let result = goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 9, 34).await;

    assert!(
        result.is_some(),
        "GTD on ->sortable() after builder-forwarded with() should resolve to scopeSortable"
    );

    let response = result.unwrap();
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Brand.php"),
        "Scope should resolve within Brand.php, got: {}",
        uri.as_str()
    );

    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "scopeSortable is on line 5 (0-indexed), got: {}",
        line
    );
}

// ─── *_count relationship count property GTD ────────────────────────────────

#[tokio::test]
async fn test_goto_definition_count_property_jumps_to_relationship_method() {
    // Ctrl+click on `$user->posts_count` should jump to the `posts()` method.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    /** @return \\Illuminate\\Database\\Eloquent\\Relations\\HasMany<Post, $this> */
    public function posts(): \\Illuminate\\Database\\Eloquent\\Relations\\HasMany {
        return $this->hasMany(Post::class);
    }
    public function demo(): void {
        $user = new User();
        $user->posts_count;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "posts_count" on line 10, cursor within the name
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 10, 18).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->posts_count should resolve"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to posts() method on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_count_property_camel_case_relationship() {
    // Ctrl+click on `$bakery->head_baker_count` should jump to `headBaker()`.
    let bakery_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Bakery extends Model {
    /** @return \\Illuminate\\Database\\Eloquent\\Relations\\HasOne<Baker, $this> */
    public function headBaker(): \\Illuminate\\Database\\Eloquent\\Relations\\HasOne {
        return $this->hasOne(Baker::class);
    }
    public function demo(): void {
        $b = new Bakery();
        $b->head_baker_count;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Bakery.php", bakery_php)]);

    // "head_baker_count" on line 10, cursor within the name
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Bakery.php", bakery_php, 10, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $b->head_baker_count should resolve to headBaker()"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to headBaker() method on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_count_property_on_this() {
    // Ctrl+click on `$this->posts_count` inside the same model.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    /** @return \\Illuminate\\Database\\Eloquent\\Relations\\HasMany<Post, $this> */
    public function posts(): \\Illuminate\\Database\\Eloquent\\Relations\\HasMany {
        return $this->hasMany(Post::class);
    }
    public function demo(): void {
        $this->posts_count;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "$this->posts_count" on line 9, cursor within the name
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 9, 18).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $this->posts_count should resolve"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 5,
        "Should jump to posts() method on line 5, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_scope_after_static_scope_same_model() {
    // Brand::productInformation()->sortable() — GTD on sortable should
    // jump to scopeSortable.  Both scopes are on the same model and
    // productInformation is called statically (not via Builder chain).
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    /**
     * @param Builder<self> $query
     * @return Builder<self>
     */
    public function scopeProductInformation(Builder $query): Builder {
        return $query;
    }
    public function scopeSortable(Builder $query, $defaultParameters = null): Builder {
        return $query;
    }
    public function demo(): void {
        Brand::productInformation()->sortable();
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/Brand.php", brand_php)]);

    // Cursor on "sortable" in `Brand::productInformation()->sortable();`
    // Line 16 (0-indexed): "        Brand::productInformation()->sortable();"
    // "sortable" starts at character 37
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 16, 39).await;

    assert!(
        result.is_some(),
        "Go-to-definition on ->sortable() after static scope should resolve to scopeSortable"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    assert_eq!(
        line, 12,
        "scopeSortable is on line 12 (0-indexed), got: {}",
        line
    );
}

// ── Timestamp property go-to-definition ─────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_timestamp_default_created_at() {
    // Ctrl+click on `$user->created_at` should jump to the CREATED_AT
    // constant on the parent Model class.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    protected $fillable = ['name'];
    public function demo(): void {
        $user = new User();
        $user->created_at;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "created_at" on line 7, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 7, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->created_at should resolve to CREATED_AT constant"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    // CREATED_AT is on line 3 of Model.php (0-indexed)
    assert_eq!(
        line, 3,
        "Should jump to CREATED_AT constant, got line: {}",
        line
    );
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Model.php"),
        "Should jump to Model.php, got: {}",
        uri
    );
}

#[tokio::test]
async fn test_goto_definition_timestamp_custom_created_at() {
    // When the model overrides CREATED_AT, go-to-definition should
    // jump to the constant on the model itself, not the parent.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    const CREATED_AT = 'created';
    public function demo(): void {
        $user = new User();
        $user->created;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "created" on line 7, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 7, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->created should resolve to CREATED_AT constant on User"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    // CREATED_AT is on line 4 of User.php (0-indexed)
    assert_eq!(
        line, 4,
        "Should jump to CREATED_AT constant on User, got line: {}",
        line
    );
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("User.php"),
        "Should jump to User.php (not Model.php), got: {}",
        uri
    );
}

#[tokio::test]
async fn test_goto_definition_timestamp_updated_at() {
    // Ctrl+click on `$user->updated_at` should jump to the UPDATED_AT
    // constant on the parent Model class.
    let user_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {
    public function demo(): void {
        $user = new User();
        $user->updated_at;
    }
}
";
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php)]);

    // "updated_at" on line 6, cursor at character 15
    let result = goto_definition_at(&backend, &dir, "src/Models/User.php", user_php, 6, 15).await;

    assert!(
        result.is_some(),
        "Go-to-definition on $user->updated_at should resolve to UPDATED_AT constant"
    );

    let response = result.unwrap();
    let line = definition_line(&response);
    // UPDATED_AT is on line 4 of Model.php (0-indexed)
    assert_eq!(
        line, 4,
        "Should jump to UPDATED_AT constant, got line: {}",
        line
    );
    let uri = definition_uri(&response);
    assert!(
        uri.as_str().contains("Model.php"),
        "Should jump to Model.php, got: {}",
        uri
    );
}

#[tokio::test]
async fn test_goto_definition_scope_from_trait_after_scope_chain() {
    // Brand::productInformation()->sortable() — GTD on sortable should
    // jump to scopeSortable on the Sortable trait.  The scope comes from
    // a trait used by the model, not defined directly on the model.
    let sortable_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Builder;
trait Sortable {
    public function scopeSortable(Builder $query, $defaultParameters = null): Builder {
        return $query;
    }
}
";
    let brand_php = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
use Illuminate\\Database\\Eloquent\\Builder;
class Brand extends Model {
    use Sortable;
    /**
     * @param Builder<self> $query
     * @return Builder<self>
     */
    public function scopeProductInformation(Builder $query): Builder {
        return $query;
    }
    public function demo(): void {
        Brand::productInformation()->sortable();
    }
}
";
    let (backend, dir) = make_workspace(&[
        ("src/Models/Sortable.php", sortable_php),
        ("src/Models/Brand.php", brand_php),
    ]);

    // Cursor on "sortable" in `Brand::productInformation()->sortable();`
    // Line 14 (0-indexed): "        Brand::productInformation()->sortable();"
    // "sortable" starts at character 37
    let result =
        goto_definition_at(&backend, &dir, "src/Models/Brand.php", brand_php, 14, 39).await;

    assert!(
        result.is_some(),
        "Go-to-definition on ->sortable() after scope chain should resolve to scopeSortable on trait"
    );

    let response = result.unwrap();
    let target_uri = definition_uri(&response);
    assert!(
        target_uri.as_str().ends_with("Sortable.php"),
        "Should jump to Sortable.php, got: {}",
        target_uri
    );
    let line = definition_line(&response);
    assert_eq!(
        line, 4,
        "scopeSortable is on line 4 (0-indexed) in Sortable.php, got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_config_usage_to_config_file_key() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $name = config('app.name');
        $same = \\Config::get('app.name');
    }
}
";
    let config_app_php = "\
<?php
return [
    'name' => env('APP_NAME', 'Laravel'),
    'timezone' => 'UTC',
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/app.php", config_app_php),
    ]);

    // Cursor on "app.name" in config('app.name').
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        24,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on config('app.name') should resolve to config/app.php key"
    );

    let response = result.unwrap();
    let target_uri = definition_uri(&response);
    assert!(
        target_uri.as_str().ends_with("/config/app.php"),
        "Should jump to config/app.php, got: {}",
        target_uri
    );
    let line = definition_line(&response);
    assert_eq!(
        line, 2,
        "'name' key in config/app.php is on line 2 (0-indexed), got: {}",
        line
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_config_nested_key() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $from = config('app.mail.from.address');
    }
}
";
    let config_app_php = "\
<?php
return [
    'mail' => [
        'from' => [
            'address' => 'noreply@example.com',
        ],
    ],
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/app.php", config_app_php),
    ]);

    // Cursor on "app.mail.from.address".
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        24,
    )
    .await;

    assert!(
        result.is_some(),
        "Go-to-definition on nested config key should resolve to config/app.php nested key"
    );

    let response = result.unwrap();
    let target_uri = definition_uri(&response);
    assert!(
        target_uri.as_str().ends_with("/config/app.php"),
        "Should jump to config/app.php, got: {}",
        target_uri
    );
    let line = definition_line(&response);
    assert_eq!(
        line, 4,
        "'address' key should be on line 4 (0-indexed), got: {}",
        line
    );
}

// ─── Laravel config Find All References ─────────────────────────────────────

/// Open `relative_path` via `did_open` and call `find_references` at the given position.
async fn find_references_at(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    relative_path: &str,
    content: &str,
    line: u32,
    character: u32,
    include_declaration: bool,
) -> Option<Vec<tower_lsp::lsp_types::Location>> {
    let uri = Url::from_file_path(dir.path().join(relative_path)).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: content.to_string(),
            },
        })
        .await;

    backend.find_references(
        uri.as_str(),
        content,
        Position { line, character },
        include_declaration,
    )
}

#[tokio::test]
async fn test_find_references_laravel_config_from_usage_site() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $name = config('app.name');
        $same = \\Config::get('app.name');
        \\Config::set('app.name', 'NewApp');
    }
}
";
    // config/app.php — 'name' is on line 2 (0-indexed)
    let config_app_php = "\
<?php
return [
    'name' => 'Laravel',
    'timezone' => 'UTC',
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/app.php", config_app_php),
    ]);

    // Cursor on "app.name" in config('app.name') — line 4, char 24.
    let results = find_references_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        24,
        true,
    )
    .await;

    let results = results.expect("find_references should return locations for config key");

    // 3 usages (config(), Config::get(), Config::set()) + 1 declaration in config/app.php
    assert_eq!(
        results.len(),
        4,
        "Expected 3 usages + 1 declaration = 4, got {}: {:#?}",
        results.len(),
        results
    );

    let has_declaration = results
        .iter()
        .any(|l| l.uri.as_str().ends_with("/config/app.php"));
    assert!(
        has_declaration,
        "Expected a reference pointing to config/app.php"
    );
}

#[tokio::test]
async fn test_find_references_laravel_config_exclude_declaration() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $name = config('app.name');
        $same = \\Config::get('app.name');
        \\Config::set('app.name', 'NewApp');
    }
}
";
    let config_app_php = "\
<?php
return [
    'name' => 'Laravel',
    'timezone' => 'UTC',
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/app.php", config_app_php),
    ]);

    // include_declaration = false: should return 3 usages only.
    let results = find_references_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        24,
        false,
    )
    .await;

    let results = results.expect("find_references should return locations");
    assert_eq!(
        results.len(),
        3,
        "Expected 3 usages only (no declaration), got {}: {:#?}",
        results.len(),
        results
    );

    let has_config_file = results
        .iter()
        .any(|l| l.uri.as_str().ends_with("/config/app.php"));
    assert!(
        !has_config_file,
        "Should not include config/app.php when include_declaration=false"
    );
}

#[tokio::test]
async fn test_find_references_laravel_config_from_declaration_site() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $name = config('app.name');
        $same = \\Config::get('app.name');
        \\Config::set('app.name', 'NewApp');
    }
}
";
    // 'name' key is on line 2, char 5 (inside quotes).
    let config_app_php = "\
<?php
return [
    'name' => 'Laravel',
    'timezone' => 'UTC',
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/app.php", config_app_php),
    ]);

    // Open service.php so it is indexed before querying from config/app.php.
    let service_uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: service_uri,
                language_id: "php".to_string(),
                version: 1,
                text: service_php.to_string(),
            },
        })
        .await;

    // Cursor on "name" key inside config/app.php — line 2, char 5.
    let results =
        find_references_at(&backend, &dir, "config/app.php", config_app_php, 2, 5, true).await;

    let results = results.expect("find_references from declaration site should return locations");

    // 3 usages + 1 declaration
    assert_eq!(
        results.len(),
        4,
        "Expected 3 usages + 1 declaration = 4 from declaration site, got {}: {:#?}",
        results.len(),
        results
    );
}

#[tokio::test]
async fn test_find_references_laravel_config_nested() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $key = config('api.keys.secret');
    }
}
";
    // config/api/keys.php
    let config_api_keys_php = "\
<?php
return [
    'secret' => 'top-secret',
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/api/keys.php", config_api_keys_php),
    ]);

    // Open service.php so it is indexed.
    let service_uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: service_uri,
                language_id: "php".to_string(),
                version: 1,
                text: service_php.to_string(),
            },
        })
        .await;

    // Cursor on \"secret\" key inside config/api/keys.php — line 2, char 5.
    let results = find_references_at(
        &backend,
        &dir,
        "config/api/keys.php",
        config_api_keys_php,
        2,
        5,
        true,
    )
    .await;

    let results = results.expect("find_references from nested config should return locations");

    // 1 usage + 1 declaration
    assert_eq!(
        results.len(),
        2,
        "Expected 1 usage + 1 declaration = 2 from nested config, got {}: {:#?}",
        results.len(),
        results
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_config_typed_accessors() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $a = \\Config::string('app.name');
        $b = \\Config::has('app.timezone');
        $c = \\Config::integer('app.retry');
        $d = \\Config::boolean('app.debug');
        $e = \\Config::float('app.rate');
        $f = \\Config::array('app.providers');
        $g = \\Config::collection('app.providers');
        $h = \\Config::prepend('app.providers', 'X');
        $i = \\Config::push('app.providers', 'Y');
    }
}
";
    let config_app_php = "\
<?php
return [
    'name' => 'Laravel',
    'timezone' => 'UTC',
    'retry' => 3,
    'debug' => true,
    'rate' => 1.5,
    'providers' => [],
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/app.php", config_app_php),
    ]);

    // Config::string('app.name') — cursor on "app.name"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        35,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::string('app.name') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(definition_line(&response), 2, "Config::string → 'name' key");

    // Config::has('app.timezone') — cursor on "app.timezone"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        5,
        30,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::has('app.timezone') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        3,
        "Config::has → 'timezone' key"
    );

    // Config::integer('app.retry') — cursor on "app.retry"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        6,
        36,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::integer('app.retry') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        4,
        "Config::integer → 'retry' key"
    );

    // Config::boolean('app.debug') — cursor on "app.debug"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        7,
        36,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::boolean('app.debug') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        5,
        "Config::boolean → 'debug' key"
    );

    // Config::float('app.rate') — cursor on "app.rate"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        8,
        33,
    )
    .await;
    assert!(result.is_some(), "Config::float('app.rate') should resolve");
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(definition_line(&response), 6, "Config::float → 'rate' key");

    // Config::array('app.providers') — cursor on "app.providers"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        9,
        33,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::array('app.providers') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        7,
        "Config::array → 'providers' key"
    );

    // Config::collection('app.providers') — cursor on "app.providers"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        10,
        34,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::collection('app.providers') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        7,
        "Config::collection → 'providers' key"
    );

    // Config::prepend('app.providers', 'X') — cursor on "app.providers"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        11,
        31,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::prepend('app.providers') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        7,
        "Config::prepend → 'providers' key"
    );

    // Config::push('app.providers', 'Y') — cursor on "app.providers"
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        12,
        28,
    )
    .await;
    assert!(
        result.is_some(),
        "Config::push('app.providers') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        7,
        "Config::push → 'providers' key"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_config_helper_typed_accessors() {
    let service_php = "\
<?php
class Service {
    public function demo(): void {
        $a = config()->get('app.name');
        $b = config()->has('app.timezone');
        $c = config()->string('app.name');
        $d = config()->integer('app.retry');
        $e = config()->float('app.rate');
        $f = config()->boolean('app.debug');
        $g = config()->array('app.providers');
        $h = config()->collection('app.providers');
        $i = config()->set('app.name', 'X');
        $j = config()->prepend('app.providers', 'X');
        $k = config()->push('app.providers', 'Y');
        $l = \\config()->get('app.name');
    }
}
";
    let config_app_php = "\
<?php
return [
    'name' => 'Laravel',
    'timezone' => 'UTC',
    'retry' => 3,
    'debug' => true,
    'rate' => 1.5,
    'providers' => [],
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Service.php", service_php),
        ("config/app.php", config_app_php),
    ]);

    // config()->get('app.name') — cursor on "app.name"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 3, 28).await;
    assert!(result.is_some(), "config()->get('app.name') should resolve");
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(definition_line(&response), 2, "config()->get → 'name' key");

    // config()->has('app.timezone') — cursor on "app.timezone"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 4, 28).await;
    assert!(
        result.is_some(),
        "config()->has('app.timezone') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        3,
        "config()->has → 'timezone' key"
    );

    // config()->string('app.name') — cursor on "app.name"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 5, 31).await;
    assert!(
        result.is_some(),
        "config()->string('app.name') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        2,
        "config()->string → 'name' key"
    );

    // config()->integer('app.retry') — cursor on "app.retry"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 6, 32).await;
    assert!(
        result.is_some(),
        "config()->integer('app.retry') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        4,
        "config()->integer → 'retry' key"
    );

    // config()->float('app.rate') — cursor on "app.rate"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 7, 30).await;
    assert!(
        result.is_some(),
        "config()->float('app.rate') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        6,
        "config()->float → 'rate' key"
    );

    // config()->boolean('app.debug') — cursor on "app.debug"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 8, 32).await;
    assert!(
        result.is_some(),
        "config()->boolean('app.debug') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        5,
        "config()->boolean → 'debug' key"
    );

    // config()->array('app.providers') — cursor on "app.providers"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 9, 30).await;
    assert!(
        result.is_some(),
        "config()->array('app.providers') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        7,
        "config()->array → 'providers' key"
    );

    // config()->collection('app.providers') — cursor on "app.providers"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 10, 35).await;
    assert!(
        result.is_some(),
        "config()->collection('app.providers') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        7,
        "config()->collection → 'providers' key"
    );

    // config()->set('app.name', 'X') — cursor on "app.name"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 11, 28).await;
    assert!(result.is_some(), "config()->set('app.name') should resolve");
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(definition_line(&response), 2, "config()->set → 'name' key");

    // config()->prepend('app.providers', 'X') — cursor on "app.providers"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 12, 32).await;
    assert!(
        result.is_some(),
        "config()->prepend('app.providers') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        7,
        "config()->prepend → 'providers' key"
    );

    // \config()->get('app.name') — cursor on "app.name"
    let result = goto_definition_at(&backend, &dir, "src/Service.php", service_php, 14, 29).await;
    assert!(
        result.is_some(),
        "\\config()->get('app.name') should resolve"
    );
    let response = result.unwrap();
    assert!(
        definition_uri(&response)
            .as_str()
            .ends_with("/config/app.php")
    );
    assert_eq!(
        definition_line(&response),
        2,
        "\\config()->get → 'name' key"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_config_nested() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $key = config('api.keys.secret');
    }
}
";
    // config/api/keys.php
    let config_api_keys_php = "\
<?php
return [
    'secret' => 'top-secret',
];
";
    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("config/api/keys.php", config_api_keys_php),
    ]);

    // Cursor on "api.keys.secret" — line 4, char 24.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        24,
    )
    .await;

    let result = result.expect("Should find definition for nested config key");
    let target_uri = definition_uri(&result);
    assert!(target_uri.as_str().ends_with("/config/api/keys.php"));
    // 'secret' is on line 2 (0-indexed)
    assert_eq!(definition_line(&result), 2);
}

// ─── env() go-to-definition ─────────────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_laravel_env_basic() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $key = env('APP_KEY');
    }
}
";
    // APP_KEY is on line 1 (0-indexed)
    let dot_env = "APP_NAME=Laravel\nAPP_KEY=base64:abc123\nDB_HOST=127.0.0.1\n";

    let (backend, dir) =
        make_workspace(&[("src/Services/Service.php", service_php), (".env", dot_env)]);

    // Cursor on "APP_KEY" in env('APP_KEY') — line 4, char 20.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        20,
    )
    .await;

    let result = result.expect("Go-to-definition on env('APP_KEY') should resolve to .env");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/.env"),
        "Should jump to .env, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        1,
        "APP_KEY is on line 1 (0-indexed) in .env"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_env_with_default() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $host = env('DB_HOST', 'localhost');
    }
}
";
    // DB_HOST is on line 2 (0-indexed)
    let dot_env = "APP_NAME=Laravel\nAPP_KEY=base64:abc123\nDB_HOST=127.0.0.1\n";

    let (backend, dir) =
        make_workspace(&[("src/Services/Service.php", service_php), (".env", dot_env)]);

    // Cursor on "DB_HOST" in env('DB_HOST', 'localhost') — line 4, char 21.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        21,
    )
    .await;

    let result =
        result.expect("Go-to-definition on env() with default should still resolve to .env");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/.env"),
        "Should jump to .env, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        2,
        "DB_HOST is on line 2 (0-indexed) in .env"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_env_missing_key_falls_back_to_line_zero() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $v = env('UNDEFINED_KEY');
    }
}
";
    let dot_env = "APP_NAME=Laravel\nAPP_KEY=base64:abc123\n";

    let (backend, dir) =
        make_workspace(&[("src/Services/Service.php", service_php), (".env", dot_env)]);

    // Cursor on "UNDEFINED_KEY" — line 4, char 18.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        18,
    )
    .await;

    // A result is still returned (pointing to .env line 0) so the editor
    // opens the file even when the key is absent.
    let result = result.expect("Should still return a location pointing to .env line 0");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/.env"),
        "Should jump to .env, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        0,
        "Unknown key falls back to line 0"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_env_no_dotenv_file_returns_none() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $key = env('APP_KEY');
    }
}
";
    // No .env file in the workspace.
    let (backend, dir) = make_workspace(&[("src/Services/Service.php", service_php)]);

    // Cursor on "APP_KEY" — line 4, char 20.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        20,
    )
    .await;

    assert!(
        result.is_none(),
        "Should return None when .env does not exist"
    );
}

// ─── view() go-to-definition ────────────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_laravel_view_simple() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $v = view('welcome');
    }
}
";
    let blade = "<!-- Welcome page -->";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("resources/views/welcome.blade.php", blade),
    ]);

    // Cursor on "welcome" in view('welcome') — line 4, char 19.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        19,
    )
    .await;

    let result = result.expect("view('welcome') should resolve to welcome.blade.php");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri
            .as_str()
            .ends_with("/resources/views/welcome.blade.php"),
        "Should jump to resources/views/welcome.blade.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        0,
        "Should point to line 0 of the blade file"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_view_nested() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $v = view('components.button');
    }
}
";
    let blade = "<!-- Button component -->";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("resources/views/components/button.blade.php", blade),
    ]);

    // Cursor on "components.button" — line 4, char 19.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        19,
    )
    .await;

    let result = result.expect("view('components.button') should resolve to blade template");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri
            .as_str()
            .ends_with("/resources/views/components/button.blade.php"),
        "Should jump to components/button.blade.php, got: {}",
        target_uri
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_view_custom_path() {
    // `config/view.php` registers a non-default view directory. A
    // `view()` name should resolve to a template living under that
    // custom root, not just `resources/views`.
    let view_config = "\
<?php
return [
    'paths' => [
        realpath(base_path('resources/backoffice/views')),
        realpath(base_path('resources/views')),
    ],
];
";
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $v = view('admin.permissions.index');
    }
}
";
    let blade = "<!-- Permissions index -->";

    let (backend, dir) = make_workspace(&[
        ("config/view.php", view_config),
        ("src/Services/Service.php", service_php),
        (
            "resources/backoffice/views/admin/permissions/index.blade.php",
            blade,
        ),
    ]);

    // Cursor on "admin.permissions.index" — line 4, char 20.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        20,
    )
    .await;

    let result =
        result.expect("view('admin.permissions.index') should resolve under the custom root");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri
            .as_str()
            .ends_with("/resources/backoffice/views/admin/permissions/index.blade.php"),
        "Should jump to the template under the custom view root, got: {}",
        target_uri
    );
}

#[tokio::test]
async fn test_view_diagnostic_respects_custom_path() {
    // A view under a custom root (from `config/view.php`) must not be
    // flagged as unknown, while a genuinely missing view still is.
    let view_config = "\
<?php
return [
    'paths' => [
        realpath(base_path('resources/backoffice/views')),
        realpath(base_path('resources/views')),
    ],
];
";
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        view('admin.permissions.index');
        view('does.not.exist');
    }
}
";
    let blade = "<!-- Permissions index -->";

    let (backend, dir) = make_workspace(&[
        ("config/view.php", view_config),
        ("src/Services/Service.php", service_php),
        (
            "resources/backoffice/views/admin/permissions/index.blade.php",
            blade,
        ),
    ]);

    let uri = Url::from_file_path(dir.path().join("src/Services/Service.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: service_php.to_string(),
            },
        })
        .await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), service_php, &mut diags);

    let view_diags: Vec<&Diagnostic> = diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(s)) if s == "invalid_laravel_view"),
        )
        .collect();

    assert_eq!(
        view_diags.len(),
        1,
        "Only the missing view should be flagged, got: {:?}",
        view_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert!(
        view_diags[0].message.contains("does.not.exist"),
        "The flagged view should be the missing one, got: {}",
        view_diags[0].message
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_view_facade() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $v = View::make('admin.index');
    }
}
";
    let blade = "<!-- Admin index -->";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("resources/views/admin/index.blade.php", blade),
    ]);

    // Cursor on "admin.index" in View::make('admin.index') — line 4, char 25.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        25,
    )
    .await;

    let result = result.expect("View::make('admin.index') should resolve to blade template");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri
            .as_str()
            .ends_with("/resources/views/admin/index.blade.php"),
        "Should jump to admin/index.blade.php, got: {}",
        target_uri
    );
}

// ─── route() go-to-definition ───────────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_laravel_route_simple() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $url = route('home');
    }
}
";
    let routes_web = "\
<?php
Route::get('/')->name('home');
Route::get('/user/profile')->name('user.profile');
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("routes/web.php", routes_web),
    ]);

    // Cursor on "home" in route('home') — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result = result.expect("route('home') should resolve to ->name('home') in routes/web.php");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/routes/web.php"),
        "Should jump to routes/web.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        1,
        "->name('home') is on line 1 (0-indexed) in routes/web.php"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_route_dotted() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $url = route('user.profile');
    }
}
";
    let routes_web = "\
<?php
Route::get('/')->name('home');
Route::get('/user/profile')->name('user.profile');
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("routes/web.php", routes_web),
    ]);

    // Cursor on "user.profile" in route('user.profile') — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result =
        result.expect("route('user.profile') should resolve to ->name('user.profile') in routes");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/routes/web.php"),
        "Should jump to routes/web.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        2,
        "->name('user.profile') is on line 2 (0-indexed) in routes/web.php"
    );
}

// ─── __() / trans() / Lang::get() go-to-definition ──────────────────────────

#[tokio::test]
async fn test_goto_definition_laravel_trans_basic() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $msg = __('messages.welcome');
    }
}
";
    let messages_php = "\
<?php
return [
    'welcome' => 'Welcome!',
    'auth' => [
        'failed' => 'Auth failed.',
        'password' => 'Wrong password.',
    ],
];
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("lang/en/messages.php", messages_php),
    ]);

    // Cursor on "messages.welcome" in __('messages.welcome') — line 4, char 19.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        19,
    )
    .await;

    let result = result.expect("__('messages.welcome') should resolve to lang/en/messages.php");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/lang/en/messages.php"),
        "Should jump to lang/en/messages.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        2,
        "'welcome' key is on line 2 (0-indexed) in lang/en/messages.php"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_trans_lang_facade() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $msg = \\Lang::get('auth.failed');
    }
}
";
    let auth_php = "\
<?php
return [
    'failed' => 'Auth failed.',
    'password' => 'Wrong password.',
];
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("lang/en/auth.php", auth_php),
    ]);

    // Cursor on "auth.failed" in \Lang::get('auth.failed') — line 4, char 27.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        27,
    )
    .await;

    let result = result.expect("Lang::get('auth.failed') should resolve to lang/en/auth.php");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/lang/en/auth.php"),
        "Should jump to lang/en/auth.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        2,
        "'failed' key is on line 2 (0-indexed) in lang/en/auth.php"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_trans_nested_key() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $msg = trans('messages.auth.password');
    }
}
";
    let messages_php = "\
<?php
return [
    'welcome' => 'Welcome!',
    'auth' => [
        'failed' => 'Auth failed.',
        'password' => 'Wrong password.',
    ],
];
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("lang/en/messages.php", messages_php),
    ]);

    // Cursor on "messages.auth.password" in trans('messages.auth.password') — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result =
        result.expect("trans('messages.auth.password') should resolve to lang/en/messages.php");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/lang/en/messages.php"),
        "Should jump to lang/en/messages.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        5,
        "'password' nested key is on line 5 (0-indexed) in lang/en/messages.php"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_route_group_name_prefix() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $url = route('admin.email.template.create');
    }
}
";
    // Route is declared with a single group name prefix, not a full explicit name.
    let routes_web = "\
<?php
Route::name('admin.email.template.')->group(function () {
    Route::get('/create', function () {})->name('create');
    Route::get('/edit', function () {})->name('edit');
});
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("routes/web.php", routes_web),
    ]);

    // Cursor on "admin.email.template.create" — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result =
        result.expect("route('admin.email.template.create') should resolve via group name prefix");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/routes/web.php"),
        "Should jump to routes/web.php, got: {}",
        target_uri
    );
    // ->name('create') is on line 2 of routes/web.php.
    assert_eq!(
        definition_line(&result),
        2,
        "->name('create') inside group is on line 2 (0-indexed) in routes/web.php"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_route_nested_group_prefix() {
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $url = route('admin.email.template.create');
    }
}
";
    // Route is declared with nested groups, each adding a name prefix.
    let routes_web = "\
<?php
Route::name('admin.')->group(function () {
    Route::name('email.template.')->group(function () {
        Route::get('/create', function () {})->name('create');
    });
});
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("routes/web.php", routes_web),
    ]);

    // Cursor on "admin.email.template.create" — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result =
        result.expect("Nested group prefixes should assemble the full route name correctly");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/routes/web.php"),
        "Should jump to routes/web.php, got: {}",
        target_uri
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_route_static_group_call() {
    // Route::group([options], fn(){}) — static call, common old-style pattern.
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $url = route('posts.show');
    }
}
";
    let routes_web = "\
<?php
Route::group(['middleware' => 'web'], function () {
    Route::get('/posts/{id}', [PostController::class, 'show'])->name('posts.show');
});
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("routes/web.php", routes_web),
    ]);

    // Cursor on "posts.show" — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result =
        result.expect("route('posts.show') inside Route::group([...], fn) should be resolved");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/routes/web.php"),
        "Should jump to routes/web.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        2,
        "->name('posts.show') is on line 2 (0-indexed)"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_route_static_group_with_as_prefix() {
    // Route::group(['as' => 'admin.', ...], fn(){}) — static call with array 'as' prefix.
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $url = route('admin.dashboard');
    }
}
";
    let routes_web = "\
<?php
Route::group(['as' => 'admin.', 'prefix' => 'admin', 'middleware' => 'auth'], function () {
    Route::get('/dashboard', 'DashboardController@index')->name('dashboard');
});
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("routes/web.php", routes_web),
    ]);

    // Cursor on "admin.dashboard" — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result =
        result.expect("Route::group(['as'=>'admin.', ...], fn) should resolve 'admin.dashboard'");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/routes/web.php"),
        "Should jump to routes/web.php, got: {}",
        target_uri
    );
    assert_eq!(
        definition_line(&result),
        2,
        "->name('dashboard') is on line 2 (0-indexed)"
    );
}

#[tokio::test]
async fn test_goto_definition_laravel_route_in_subdirectory() {
    // Routes can live in subdirectories like routes/web/, routes/api/, etc.
    let service_php = "\
<?php
namespace App\\Services;
class Service {
    public function demo(): void {
        $url = route('products.index');
    }
}
";
    let routes_products = "\
<?php
Route::get('/products', [ProductController::class, 'index'])->name('products.index');
";

    let (backend, dir) = make_workspace(&[
        ("src/Services/Service.php", service_php),
        ("routes/web/products.php", routes_products),
    ]);

    // Cursor on "products.index" — line 4, char 22.
    let result = goto_definition_at(
        &backend,
        &dir,
        "src/Services/Service.php",
        service_php,
        4,
        22,
    )
    .await;

    let result = result.expect("route in routes/web/ subdirectory should be found recursively");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri.as_str().ends_with("/routes/web/products.php"),
        "Should jump to routes/web/products.php, got: {}",
        target_uri
    );
    assert_eq!(definition_line(&result), 1);
}

// ─── Blade @include go-to-definition ─────────────────────────────────────────

#[tokio::test]
async fn test_goto_definition_blade_include_directive() {
    let blade_layout = "<div>\n    @include('partials.header')\n</div>";
    let blade_partial = "<!-- header partial -->";

    let (backend, dir) = make_workspace(&[
        ("resources/views/layout.blade.php", blade_layout),
        ("resources/views/partials/header.blade.php", blade_partial),
    ]);

    // Cursor on "partials.header" in @include('partials.header') — line 1, char 16.
    let result = goto_definition_at(
        &backend,
        &dir,
        "resources/views/layout.blade.php",
        blade_layout,
        1,
        16,
    )
    .await;

    let result = result.expect("@include('partials.header') should resolve to blade template");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri
            .as_str()
            .ends_with("/resources/views/partials/header.blade.php"),
        "Should jump to partials/header.blade.php, got: {}",
        target_uri
    );
}

#[tokio::test]
async fn test_goto_definition_blade_extends_directive() {
    let blade_child = "@extends('layouts.app')\n<p>Content</p>";
    let blade_layout = "<!-- app layout -->";

    let (backend, dir) = make_workspace(&[
        ("resources/views/page.blade.php", blade_child),
        ("resources/views/layouts/app.blade.php", blade_layout),
    ]);

    // Cursor on "layouts.app" in @extends('layouts.app') — line 0, char 13.
    let result = goto_definition_at(
        &backend,
        &dir,
        "resources/views/page.blade.php",
        blade_child,
        0,
        13,
    )
    .await;

    let result = result.expect("@extends('layouts.app') should resolve to blade template");
    let target_uri = definition_uri(&result);
    assert!(
        target_uri
            .as_str()
            .ends_with("/resources/views/layouts/app.blade.php"),
        "Should jump to layouts/app.blade.php, got: {}",
        target_uri
    );
}
