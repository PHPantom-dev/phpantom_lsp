//! Eloquent magic members seen from call sites: scopes, accessors, columns,
//! relationships, factories, and the builder chains that carry them.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.

use crate::common::{
    create_psr4_workspace, definition_locations, goto_definition_at, method_names, open_php,
    property_names, split_cursor,
};
use tower_lsp::lsp_types::*;

// ─── Framework stubs ────────────────────────────────────────────────────────

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\Models\\": "src/Models/",
            "App\\Concerns\\": "src/Concerns/",
            "Database\\Factories\\": "database/factories/",
            "Illuminate\\Support\\": "vendor/illuminate/Support/",
            "Illuminate\\Support\\Traits\\": "vendor/illuminate/Support/Traits/",
            "Illuminate\\Database\\Eloquent\\": "vendor/illuminate/Eloquent/",
            "Illuminate\\Database\\Eloquent\\Casts\\": "vendor/illuminate/Eloquent/Casts/",
            "Illuminate\\Database\\Eloquent\\Factories\\": "vendor/illuminate/Eloquent/Factories/",
            "Illuminate\\Database\\Eloquent\\Relations\\": "vendor/illuminate/Eloquent/Relations/",
            "Illuminate\\Database\\Query\\": "vendor/illuminate/Query/",
            "Illuminate\\Database\\Concerns\\": "vendor/illuminate/Concerns/",
            "Illuminate\\Pagination\\": "vendor/illuminate/Pagination/"
        }
    }
}"#;

const MODEL_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
use Illuminate\Database\Eloquent\Relations\HasOne;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
class Model {
    protected $guarded = ['*'];
    /** @return \Illuminate\Database\Eloquent\Builder<static> */
    public static function query() { return new Builder(); }
    /** @return \Illuminate\Database\Eloquent\Builder<static> */
    public function newQuery() { return new Builder(); }
    /** @return \Illuminate\Database\Eloquent\Collection<int, static> */
    public static function all($columns = ['*']) { return new Collection(); }
    /**
     * @template TRelatedModel of Model
     * @param class-string<TRelatedModel> $related
     * @return HasOne<TRelatedModel, $this>
     */
    public function hasOne($related, $foreignKey = null, $localKey = null) {}
    /**
     * @template TRelatedModel of Model
     * @param class-string<TRelatedModel> $related
     * @return HasMany<TRelatedModel, $this>
     */
    public function hasMany($related, $foreignKey = null, $localKey = null) {}
    /**
     * @template TRelatedModel of Model
     * @param class-string<TRelatedModel> $related
     * @return BelongsTo<TRelatedModel, $this>
     */
    public function belongsTo($related, $foreignKey = null, $ownerKey = null, $relation = null) {}
}
"#;

const COLLECTION_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
/**
 * @template TKey of array-key
 * @template TModel
 * @implements \IteratorAggregate<TKey, TModel>
 */
class Collection implements \IteratorAggregate {
    /** @return int */
    public function count(): int { return 0; }
    /** @return TModel|null */
    public function first(): mixed { return null; }
    /** @return static */
    public function where($key, $operator = null, $value = null): static { return $this; }
    /** @return \ArrayIterator<TKey, TModel> */
    public function getIterator(): \ArrayIterator { return new \ArrayIterator([]); }
}
"#;

const FORWARDS_CALLS_TRAIT_PHP: &str = r#"<?php
namespace Illuminate\Support\Traits;
trait ForwardsCalls {
    protected function forwardCallTo(mixed $object, string $method, array $parameters): mixed { return null; }
}
"#;

const BUILDER_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;

use Illuminate\Database\Concerns\BuildsQueries;
use Illuminate\Support\Traits\ForwardsCalls;

/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 * @mixin \Illuminate\Database\Query\Builder
 */
class Builder {
    /** @use BuildsQueries<TModel> */
    use BuildsQueries, ForwardsCalls;

    /**
     * @param  (\Closure(static): mixed)|string|array  $column
     * @return $this
     */
    public function where($column, $operator = null, $value = null, $boolean = 'and') { return $this; }
    /** @return static */
    public function orderBy(string $column, string $direction = 'asc'): static { return $this; }
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
    public function get(): Collection { return new Collection(); }
    /**
     * @param  string  $relation
     * @param  (\Closure(\Illuminate\Database\Eloquent\Builder<TModel>): mixed)|null  $callback
     * @return static
     */
    public function whereHas(string $relation, ?\Closure $callback = null): static { return $this; }
}
"#;

const QUERY_BUILDER_PHP: &str = r#"<?php
namespace Illuminate\Database\Query;
class Builder {
    /** @return static */
    public function whereIn(string $column, array $values): static { return $this; }
    /** @return $this */
    public function lockForUpdate() { return $this; }
}
"#;

const BUILDS_QUERIES_PHP: &str = r#"<?php
namespace Illuminate\Database\Concerns;
/**
 * @template TValue
 */
trait BuildsQueries {
    /** @return TValue|null */
    public function first(): mixed { return null; }
}
"#;

const SUPPORT_COLLECTION_PHP: &str = r#"<?php
namespace Illuminate\Support;
/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {
    /** @return int */
    public function count(): int { return 0; }
    /** @return TValue|null */
    public function first(): mixed { return null; }
}
"#;

const RELATION_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
use Illuminate\Support\Traits\ForwardsCalls;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @template TResult
 * @mixin \Illuminate\Database\Eloquent\Builder<TRelatedModel>
 */
class Relation {
    use ForwardsCalls;
}
"#;

const HAS_ONE_OR_MANY_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @template TResult
 * @extends Relation<TRelatedModel, TDeclaringModel, TResult>
 */
class HasOneOrMany extends Relation {}
"#;

const HAS_MANY_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @extends HasOneOrMany<TRelatedModel, TDeclaringModel, \Illuminate\Database\Eloquent\Collection<int, TRelatedModel>>
 */
class HasMany extends HasOneOrMany {}
"#;

const HAS_ONE_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @extends HasOneOrMany<TRelatedModel, TDeclaringModel, TRelatedModel|null>
 */
class HasOne extends HasOneOrMany {}
"#;

const BELONGS_TO_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 * @extends Relation<TRelatedModel, TDeclaringModel, TRelatedModel|null>
 */
class BelongsTo extends Relation {}
"#;

const ATTRIBUTE_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Casts;
class Attribute {
    public static function make(?callable $get = null, ?callable $set = null): static { return new static(); }
}
"#;

const HAS_FACTORY_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Factories;
/**
 * @template TFactory of Factory
 */
trait HasFactory {
    /** @return TFactory */
    public static function factory($count = null, $state = []) {}
}
"#;

const FACTORY_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Factories;
/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 */
class Factory {
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel>|TModel */
    public function create(array $attributes = []) {}
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel>|TModel */
    public function make(array $attributes = []) {}
    /** @return static */
    public static function new(array $attributes = []): static {}
    /** @return static */
    public function state(array $state): static { return $this; }
}
"#;

const PAGINATOR_PHP: &str = r#"<?php
namespace Illuminate\Pagination;
/**
 * @template TKey of array-key
 * @template TValue
 * @implements \IteratorAggregate<TKey, TValue>
 */
class LengthAwarePaginator implements \IteratorAggregate {
    /** @return \ArrayIterator<TKey, TValue> */
    public function getIterator(): \ArrayIterator { return new \ArrayIterator([]); }
}
"#;

fn framework_stubs() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vendor/illuminate/Eloquent/Model.php", MODEL_PHP),
        ("vendor/illuminate/Eloquent/Collection.php", COLLECTION_PHP),
        ("vendor/illuminate/Eloquent/Builder.php", BUILDER_PHP),
        ("vendor/illuminate/Query/Builder.php", QUERY_BUILDER_PHP),
        (
            "vendor/illuminate/Concerns/BuildsQueries.php",
            BUILDS_QUERIES_PHP,
        ),
        (
            "vendor/illuminate/Support/Traits/ForwardsCalls.php",
            FORWARDS_CALLS_TRAIT_PHP,
        ),
        (
            "vendor/illuminate/Support/Collection.php",
            SUPPORT_COLLECTION_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Relations/Relation.php",
            RELATION_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Relations/HasOneOrMany.php",
            HAS_ONE_OR_MANY_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Relations/HasMany.php",
            HAS_MANY_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Relations/HasOne.php",
            HAS_ONE_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Relations/BelongsTo.php",
            BELONGS_TO_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Casts/Attribute.php",
            ATTRIBUTE_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Factories/HasFactory.php",
            HAS_FACTORY_PHP,
        ),
        (
            "vendor/illuminate/Eloquent/Factories/Factory.php",
            FACTORY_PHP,
        ),
        (
            "vendor/illuminate/Pagination/LengthAwarePaginator.php",
            PAGINATOR_PHP,
        ),
    ]
}

/// Build a PSR-4 workspace from the framework stubs plus extra app files.
fn make_workspace(app_files: &[(&str, &str)]) -> (phpantom_lsp::Backend, tempfile::TempDir) {
    let mut files: Vec<(&str, &str)> = framework_stubs();
    files.extend_from_slice(app_files);
    create_psr4_workspace(COMPOSER_JSON, &files)
}

/// Open `content` as the workspace file at `relative_path` and complete at
/// `position`.
async fn complete(
    backend: &phpantom_lsp::Backend,
    dir: &tempfile::TempDir,
    relative_path: &str,
    content: &str,
    position: Position,
) -> Vec<CompletionItem> {
    let uri = Url::from_file_path(dir.path().join(relative_path)).unwrap();
    crate::common::complete_at(backend, &uri, content, position.line, position.character).await
}

/// The `detail` of the property item named `name`, when there is exactly one.
fn single_property_detail(items: &[CompletionItem], name: &str) -> String {
    let matching: Vec<&CompletionItem> = items
        .iter()
        .filter(|i| {
            i.kind == Some(CompletionItemKind::PROPERTY)
                && i.filter_text.as_deref().unwrap_or(&i.label) == name
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one `{name}` property, got: {:?}",
        property_names(items)
    );
    matching[0].detail.clone().unwrap_or_default()
}

const USER_WITH_ACTIVE_SCOPE: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class User extends Model {
    protected $fillable = ['email'];
    public function scopeActive(Builder $query): void {}
}
"#;

// ─── Scopes ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn scope_is_offered_as_a_method_but_not_as_a_property() {
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class User extends Model {
    public function scopeActive(Builder $query): void {}
    public function demo() {
        $user = new User();
        $user->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php.as_str())]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let methods = method_names(&items);
    let props = property_names(&items);

    assert!(
        methods.contains(&"active"),
        "a scope is callable on a model instance, got: {methods:?}"
    );
    assert!(
        !props.contains(&"active"),
        "a scope is not readable as a property, got: {props:?}"
    );
}

#[tokio::test]
async fn goto_definition_on_static_scope_call_lands_in_declaring_trait() {
    let trait_php = r#"<?php
namespace App\Concerns;
use Illuminate\Database\Eloquent\Builder;
trait Activatable {
    public function scopeActive(Builder $query): void {}
}
"#;
    let user_php = r#"<?php
namespace App\Models;
use App\Concerns\Activatable;
use Illuminate\Database\Eloquent\Model;
class User extends Model {
    use Activatable;
}
"#;
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function index() {
        return User::act§ive()->get();
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Concerns/Activatable.php", trait_php),
        ("src/Models/User.php", user_php),
    ]);

    let uri = Url::from_file_path(dir.path().join("src/Http/Controller.php")).unwrap();
    open_php(&backend, &uri, &caller).await;
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, pos.line, pos.character).await);

    assert!(
        locations
            .first()
            .is_some_and(|l| l.uri.as_str().ends_with("Activatable.php")),
        "a trait-declared scope should resolve to the trait, got: {locations:?}"
    );
    assert_eq!(
        locations[0].range.start.line, 4,
        "should land on scopeActive in the trait, got: {locations:?}"
    );
}

#[tokio::test]
async fn scope_resolves_through_an_aliased_model_import() {
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User as Account;
class Controller {
    public function index() {
        return Account::active()->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", USER_WITH_ACTIVE_SCOPE)]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"get"),
        "a scope called through an import alias should chain into the builder, got: {methods:?}"
    );
    assert!(
        methods.contains(&"active"),
        "the builder should still carry the model's scopes, got: {methods:?}"
    );
}

#[tokio::test]
async fn scope_called_through_self_inside_the_model_chains_to_builder() {
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class User extends Model {
    public function scopeActive(Builder $query): void {}
    public function scopeVerified(Builder $query): void {}
    public static function activeUsers() {
        return self::active()->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php.as_str())]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"get"),
        "self::active() should return the model's builder, got: {methods:?}"
    );
    assert!(
        methods.contains(&"verified"),
        "the builder from self::active() should carry the other scopes, got: {methods:?}"
    );
}

#[tokio::test]
async fn scope_is_offered_after_static_query_inside_the_model() {
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class User extends Model {
    public function scopeActive(Builder $query): void {}
    public static function freshActive() {
        return static::query()->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php.as_str())]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"active"),
        "static::query() should return a builder carrying the model's scopes, got: {methods:?}"
    );
}

#[tokio::test]
async fn scope_is_offered_after_query_then_where() {
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function index() {
        return User::query()->where('email', 'x')->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", USER_WITH_ACTIVE_SCOPE)]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"active"),
        "User::query()->where(...) should keep the model's scopes, got: {methods:?}"
    );
}

#[tokio::test]
async fn scope_is_offered_mid_chain_on_the_scope_body_query_parameter() {
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class User extends Model {
    public function scopeActive(Builder $query): void {}
    public function scopeFresh(Builder $query): void {
        $query->where('x', 1)->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php.as_str())]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"active"),
        "a where() on a scope's own query parameter should keep the model's scopes, got: {methods:?}"
    );
}

#[tokio::test]
async fn where_has_closure_with_bare_builder_hint_targets_the_related_model() {
    let post_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class Post extends Model {
    public function scopeDraft(Builder $query): void {}
}
"#;
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Relations\HasMany;
class User extends Model {
    /** @return HasMany<Post, $this> */
    public function posts(): HasMany { return $this->hasMany(Post::class); }
    public function scopePublished(Builder $query): void {}
    public function scopeRecent(Builder $query): void {
        $query->whereHas('posts', function (Builder $q) {
            $q->§
        });
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/Post.php", post_php),
        ("src/Models/User.php", user_php.as_str()),
    ]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"where"),
        "the closure's query should be a builder, got: {methods:?}"
    );
    assert!(
        !methods.contains(&"published"),
        "the whereHas closure queries posts, so the enclosing model's scope must not appear, got: {methods:?}"
    );
    assert!(
        methods.contains(&"draft"),
        "the whereHas closure queries posts, so the related model's scope should appear, got: {methods:?}"
    );
}

#[tokio::test]
async fn relationship_query_does_not_offer_the_parent_models_scopes() {
    let post_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class Post extends Model {
    public function scopeDraft(Builder $query): void {}
}
"#;
    let user_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Relations\HasMany;
class User extends Model {
    /** @return HasMany<Post, $this> */
    public function posts(): HasMany { return $this->hasMany(Post::class); }
    public function scopeActive(Builder $query): void {}
}
"#;
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function show(User $user) {
        return $user->posts()->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/Post.php", post_php),
        ("src/Models/User.php", user_php),
    ]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"draft"),
        "the relationship query targets Post, so Post's scope should appear, got: {methods:?}"
    );
    assert!(
        !methods.contains(&"active"),
        "the relationship query targets Post, so User's scope must not appear, got: {methods:?}"
    );
}

// ─── Accessors and columns ──────────────────────────────────────────────────

#[tokio::test]
async fn legacy_accessor_type_wins_over_fillable_column_of_the_same_name() {
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class User extends Model {
    protected $fillable = ['name'];
    public function getNameAttribute(): string { return ''; }
    public function demo() {
        $user = new User();
        $user->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php.as_str())]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let detail = single_property_detail(&items, "name");

    assert!(
        detail.contains("string"),
        "the accessor's string type should win over the untyped fillable column, got: {detail:?}"
    );
}

#[tokio::test]
async fn modern_accessor_type_wins_over_fillable_column_of_the_same_name() {
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Casts\Attribute;
class User extends Model {
    protected $fillable = ['name'];
    /** @return Attribute<string, never> */
    protected function name(): Attribute {
        return Attribute::make(get: fn () => 'x');
    }
    public function demo() {
        $user = new User();
        $user->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php.as_str())]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let detail = single_property_detail(&items, "name");

    assert!(
        detail.contains("string"),
        "the Attribute accessor's string type should win over the untyped fillable column, got: {detail:?}"
    );
}

#[tokio::test]
async fn columns_declared_on_a_parent_model_are_inherited() {
    let base_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class BaseModel extends Model {
    protected $fillable = ['uuid'];
    protected $casts = ['is_archived' => 'boolean'];
}
"#;
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
class User extends BaseModel {
    public function demo() {
        $user = new User();
        $user->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/BaseModel.php", base_php),
        ("src/Models/User.php", user_php.as_str()),
    ]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let props = property_names(&items);

    assert!(
        props.contains(&"uuid"),
        "a fillable column declared on the parent model should be inherited, got: {props:?}"
    );
    assert!(
        props.contains(&"is_archived"),
        "a cast declared on the parent model should be inherited, got: {props:?}"
    );
}

#[tokio::test]
async fn a_subclass_inherits_each_model_setting_it_does_not_redeclare() {
    let base_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class BaseModel extends Model {
    protected $fillable = ['uuid'];
    protected $hidden = ['secret_token'];
    protected $casts = ['is_archived' => 'boolean'];
}
"#;
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
class User extends BaseModel {
    protected $hidden = [];
    protected function casts(): array {
        return ['nickname' => 'string'];
    }
    public function demo() {
        $user = new User();
        $user->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/BaseModel.php", base_php),
        ("src/Models/User.php", user_php.as_str()),
    ]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let props = property_names(&items);

    assert!(
        props.contains(&"uuid"),
        "redeclaring `$hidden` should keep the parent's `$fillable`, got: {props:?}"
    );
    assert!(
        !props.contains(&"secret_token"),
        "an empty `$hidden` on the child hides the parent's, got: {props:?}"
    );
    assert!(
        props.contains(&"nickname"),
        "the child's own `casts()` should apply, got: {props:?}"
    );
    assert!(
        single_property_detail(&items, "is_archived").contains("bool"),
        "a `casts()` method on the child merges over the parent's `$casts` property"
    );
    assert!(
        !props.contains(&"*"),
        "the framework Model's `$guarded = ['*']` is a default, not a column, got: {props:?}"
    );
}

#[tokio::test]
async fn a_date_cast_resolves_to_the_configured_date_class_inside_a_namespace() {
    let date_php = r#"<?php
namespace App\Models;
class BakeryDate {
    public function isFresh(): bool { return true; }
}
"#;
    let (invoice_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Invoice extends Model {
    protected $casts = ['paid_at' => 'datetime'];
    public function demo() {
        $this->paid_at->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/BakeryDate.php", date_php),
        ("src/Models/Invoice.php", invoice_php.as_str()),
    ]);
    *backend.laravel_date_class().write() = Some(Some("App\\Models\\BakeryDate".to_string()));

    let items = complete(&backend, &dir, "src/Models/Invoice.php", &invoice_php, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"isFresh"),
        "a `datetime` cast should resolve to the configured date class, got: {methods:?}"
    );
}

#[tokio::test]
async fn where_methods_cover_columns_declared_on_a_parent_model() {
    let base_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class BaseModel extends Model {
    protected $fillable = ['email_address'];
}
"#;
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
class User extends BaseModel {
    public function demo() {
        User::§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/BaseModel.php", base_php),
        ("src/Models/User.php", user_php.as_str()),
    ]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"whereEmailAddress"),
        "a fillable column declared on the parent model should get a where method, got: {methods:?}"
    );
}

#[tokio::test]
async fn dynamic_where_methods_follow_known_columns_and_the_where_prefix() {
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class User extends Model {
    protected $fillable = ['email_address'];
    public function demo() {
        User::§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", user_php.as_str())]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"whereEmailAddress"),
        "a multi-word column should yield a studly dynamic where, got: {methods:?}"
    );
    assert!(
        !methods.contains(&"whereNonexistent"),
        "no dynamic where should exist for an undeclared column, got: {methods:?}"
    );
    // Laravel only routes calls that start with `where` to dynamicWhere();
    // `orWhereEmailAddress()` would throw a BadMethodCallException.
    assert!(
        !methods.contains(&"orWhereEmailAddress"),
        "an or-prefixed dynamic where is not a Laravel method, got: {methods:?}"
    );
}

#[tokio::test]
async fn builder_from_new_query_does_not_expose_model_columns_as_properties() {
    // Eloquent\Builder::__get() throws for anything other than its
    // higher-order proxies and passthrough properties, so a column is not a
    // property of the builder even though it is one of the model.
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function show(User $user) {
        $q = $user->newQuery();
        $q->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", USER_WITH_ACTIVE_SCOPE)]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let methods = method_names(&items);
    let props = property_names(&items);

    assert!(
        methods.contains(&"where"),
        "newQuery() should return the model's builder, got: {methods:?}"
    );
    assert!(
        !props.contains(&"email"),
        "a model column is not a property of its builder, got: {props:?}"
    );
}

// ─── Receivers that carry a model type ──────────────────────────────────────

#[tokio::test]
async fn foreach_over_all_results_yields_model_columns() {
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function index() {
        $users = User::all();
        foreach ($users as $user) {
            $user->§
        }
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", USER_WITH_ACTIVE_SCOPE)]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let props = property_names(&items);

    assert!(
        props.contains(&"email"),
        "iterating User::all() should yield User models, got: {props:?}"
    );
}

#[tokio::test]
async fn foreach_key_value_over_query_results_yields_model_columns() {
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function index() {
        foreach (User::where('email', 'x')->get() as $i => $user) {
            $user->§
        }
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", USER_WITH_ACTIVE_SCOPE)]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let props = property_names(&items);

    assert!(
        props.contains(&"email"),
        "the value of a key => value foreach over query results should be a User, got: {props:?}"
    );
}

#[tokio::test]
async fn foreach_over_docblock_paginator_yields_model_columns() {
    let audit_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Audit extends Model {
    protected $fillable = ['event'];
}
"#;
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\Audit;
use Illuminate\Pagination\LengthAwarePaginator;
class AuditList {
    /** @return LengthAwarePaginator<int, Audit> */
    public function audits(): LengthAwarePaginator { return new LengthAwarePaginator(); }
    public function render(): void {
        foreach ($this->audits() as $audit) {
            $audit->§
        }
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/Audit.php", audit_php)]);

    let items = complete(&backend, &dir, "src/Http/AuditList.php", &caller, pos).await;
    let props = property_names(&items);

    assert!(
        props.contains(&"event"),
        "the docblock's paginator generic should type the loop variable, got: {props:?}"
    );
}

#[tokio::test]
async fn nullable_typed_model_property_exposes_the_related_models_columns() {
    let profile_php = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Profile extends Model {
    protected $fillable = ['bio'];
}
"#;
    let (user_php, pos) = split_cursor(
        r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class User extends Model {
    protected ?Profile $profile = null;
    public function bio() {
        return $this->profile->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/Profile.php", profile_php),
        ("src/Models/User.php", user_php.as_str()),
    ]);

    let items = complete(&backend, &dir, "src/Models/User.php", &user_php, pos).await;
    let props = property_names(&items);

    assert!(
        props.contains(&"bio"),
        "a ?Profile property should expose Profile's columns, got: {props:?}"
    );
}

#[tokio::test]
async fn typed_property_in_an_anonymous_class_exposes_model_columns() {
    let (component, pos) = split_cursor(
        r#"<?php
namespace App\Livewire;
use App\Models\User;
$component = new class {
    public ?User $user = null;
    public function render() {
        return $this->user->§
    }
};
"#,
    );
    let (backend, dir) = make_workspace(&[("src/Models/User.php", USER_WITH_ACTIVE_SCOPE)]);

    let items = complete(&backend, &dir, "src/Livewire/profile.php", &component, pos).await;
    let props = property_names(&items);

    assert!(
        props.contains(&"email"),
        "a typed property of an anonymous class should resolve to the model, got: {props:?}"
    );
}

// ─── Factories ──────────────────────────────────────────────────────────────

const FACTORY_USER_PHP: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Factories\HasFactory;
class User extends Model {
    use HasFactory;
    protected $fillable = ['email'];
    public function scopeActive(Builder $query): void {}
}
"#;

const USER_FACTORY_PHP: &str = r#"<?php
namespace Database\Factories;
use Illuminate\Database\Eloquent\Factories\Factory;
class UserFactory extends Factory {
    public function definition(): array { return []; }
    public function active(): static { return $this->state([]); }
}
"#;

#[tokio::test]
async fn factory_state_sharing_a_scope_name_resolves_on_the_factory() {
    let (caller, pos) = split_cursor(
        r#"<?php
namespace Tests\Feature;
use App\Models\User;
class UserTest {
    public function test_active() {
        User::factory()->active()->§
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/User.php", FACTORY_USER_PHP),
        ("database/factories/UserFactory.php", USER_FACTORY_PHP),
    ]);

    let items = complete(&backend, &dir, "tests/Feature/UserTest.php", &caller, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"definition") && methods.contains(&"create"),
        "a factory state returns the factory, not the scope's builder, got: {methods:?}"
    );
    assert!(
        !methods.contains(&"get"),
        "the factory chain must not turn into a query builder, got: {methods:?}"
    );
}

#[tokio::test]
async fn factory_resolves_for_a_fully_qualified_model_receiver() {
    let (caller, pos) = split_cursor(
        r#"<?php
function seed() {
    \App\Models\User::factory()->§
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/User.php", FACTORY_USER_PHP),
        ("database/factories/UserFactory.php", USER_FACTORY_PHP),
    ]);

    let items = complete(&backend, &dir, "database/seeders/seed.php", &caller, pos).await;
    let methods = method_names(&items);

    assert!(
        methods.contains(&"definition"),
        "a fully qualified model receiver should resolve its conventional factory, got: {methods:?}"
    );
}

// ─── Column-name strings through relationships ──────────────────────────────

const POST_WITH_TYPE_COLUMN: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Post extends Model {
    protected $fillable = ['type'];
}
"#;

const USER_WITH_POSTS: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;
class User extends Model {
    protected $fillable = ['email'];
    /** @return HasMany<Post, $this> */
    public function posts(): HasMany { return $this->hasMany(Post::class); }
}
"#;

#[tokio::test]
async fn relation_collection_where_offers_the_related_models_columns() {
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function show(User $user) {
        return $user->posts->where('§');
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/Post.php", POST_WITH_TYPE_COLUMN),
        ("src/Models/User.php", USER_WITH_POSTS),
    ]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"type"),
        "the relation's collection holds posts, so Post's columns should be offered, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"email"),
        "the parent model's columns must not leak past the relation, got: {labels:?}"
    );
}

#[tokio::test]
async fn relationship_query_where_offers_the_related_models_columns() {
    let (caller, pos) = split_cursor(
        r#"<?php
namespace App\Http;
use App\Models\User;
class Controller {
    public function show(User $user) {
        return $user->posts()->where('§');
    }
}
"#,
    );
    let (backend, dir) = make_workspace(&[
        ("src/Models/Post.php", POST_WITH_TYPE_COLUMN),
        ("src/Models/User.php", USER_WITH_POSTS),
    ]);

    let items = complete(&backend, &dir, "src/Http/Controller.php", &caller, pos).await;
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"type"),
        "the relationship queries posts, so Post's columns should be offered, got: {labels:?}"
    );
    assert!(
        !labels.contains(&"email"),
        "the parent model's columns must not leak past the relationship, got: {labels:?}"
    );
}

// ─── Hover on magic members ─────────────────────────────────────────────────

const HOVER_USER_PHP: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Casts\Attribute;
class User extends Model {
    protected $casts = ['born_at' => 'datetime'];
    /** The posts this user wrote. */
    public function posts(): HasMany { return $this->hasMany(Post::class); }
    /** The team the user belongs to. */
    public function team(): BelongsTo { return $this->belongsTo(Team::class); }
    /** Only users active in the last N days. */
    public function scopeActive(Builder $query, int $days): void {}
    /** The user's full name. */
    public function getFullNameAttribute(): string { return ''; }
    /** The name shown in the header. */
    protected function displayName(): Attribute { return Attribute::make(); }
}
"#;

const HOVER_CONSUMER_PHP: &str = r#"<?php
namespace App\Models;
class Consumer {
    public function run(User $user): void {
        $user->posts;
        $user->posts_count;
        $user->team;
        $user->full_name;
        $user->display_name;
        $user->born_at;
        User::active(7);
    }
}
"#;

/// The hover a character into `needle` in [`HOVER_CONSUMER_PHP`].
async fn magic_hover(needle: &str) -> String {
    let (backend, dir) = make_workspace(&[
        ("src/Models/User.php", HOVER_USER_PHP),
        (
            "src/Models/Post.php",
            "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model {}\n",
        ),
        (
            "src/Models/Team.php",
            "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Team extends Model {}\n",
        ),
        ("src/Models/Consumer.php", HOVER_CONSUMER_PHP),
    ]);
    let uri = Url::from_file_path(dir.path().join("src/Models/Consumer.php")).unwrap();
    open_php(&backend, &uri, HOVER_CONSUMER_PHP).await;
    let (line, character) = crate::common::line_char_of(HOVER_CONSUMER_PHP, needle);
    crate::common::markup_hover_at(&backend, &uri, line, character + 1).await
}

fn assert_mentions(hover: &str, expected: &[&str]) {
    for part in expected {
        assert!(hover.contains(part), "expected {part:?} in hover:\n{hover}");
    }
}

#[tokio::test]
async fn relationship_hover_names_the_related_model_the_body_passes() {
    assert_mentions(
        &magic_hover("posts;").await,
        &[
            "`Collection<Post>`",
            "relationship `posts`",
            "The posts this user wrote.",
        ],
    );
}

#[tokio::test]
async fn singular_relationship_hover_names_the_related_model_the_body_passes() {
    assert_mentions(
        &magic_hover("team;").await,
        &[
            "Team",
            "relationship `team`",
            "The team the user belongs to.",
        ],
    );
}

#[tokio::test]
async fn relationship_count_hover_is_an_int() {
    assert_mentions(
        &magic_hover("posts_count").await,
        &["`int`", "relationship count `posts`"],
    );
}

#[tokio::test]
async fn legacy_accessor_hover_shows_its_type_and_description() {
    assert_mentions(
        &magic_hover("full_name").await,
        &["`string`", "computed property", "The user's full name."],
    );
}

#[tokio::test]
async fn attribute_accessor_hover_shows_its_description() {
    assert_mentions(
        &magic_hover("display_name").await,
        &["computed property", "The name shown in the header."],
    );
}

#[tokio::test]
async fn cast_hover_names_the_cast() {
    assert_mentions(
        &magic_hover("born_at").await,
        &["`Carbon`", "cast `datetime`"],
    );
}

#[tokio::test]
async fn scope_hover_shows_its_description_without_the_query_parameter() {
    let hover = magic_hover("active(7)").await;
    assert_mentions(
        &hover,
        &["active(int $days)", "Only users active in the last N days."],
    );
    assert!(!hover.contains("$query"), "{hover}");
}

// ─── Builder methods offered on a model ─────────────────────────────────────

const WHERE_USER_PHP: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class User extends Model {
    protected $fillable = ['email'];
}
"#;

/// The completion items offered at the `§` in `consumer`, run against
/// the framework stubs with `replaced` stub files swapped in and the
/// `extra` app files added.
async fn complete_in(
    replaced: &[(&str, &str)],
    extra: &[(&str, &str)],
    consumer: &str,
) -> Vec<CompletionItem> {
    let (backend, _dir, uri, text, position) = open_with(replaced, extra, consumer).await;
    crate::common::complete_at(&backend, &uri, &text, position.line, position.character).await
}

/// A workspace of the framework stubs, with `replaced` stub files swapped
/// in and `extra` app files added, where `consumer` (holding a `§` cursor)
/// is open as `src/Models/Consumer.php`.
async fn open_with(
    replaced: &[(&str, &str)],
    extra: &[(&str, &str)],
    consumer: &str,
) -> (
    phpantom_lsp::Backend,
    tempfile::TempDir,
    Url,
    String,
    Position,
) {
    let (text, position) = split_cursor(consumer);
    let mut files: Vec<(&str, &str)> = framework_stubs()
        .into_iter()
        .map(|(path, content)| {
            let swapped = replaced.iter().find(|(p, _)| *p == path);
            (path, swapped.map_or(content, |(_, c)| *c))
        })
        .collect();
    files.extend_from_slice(extra);
    files.push(("src/Models/Consumer.php", text.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);
    let uri = Url::from_file_path(dir.path().join("src/Models/Consumer.php")).unwrap();
    open_php(&backend, &uri, &text).await;
    (backend, dir, uri, text, position)
}

/// The `detail` of the method item offered under `name`.
fn method_detail(items: &[CompletionItem], name: &str) -> String {
    items
        .iter()
        .find(|i| {
            i.kind == Some(CompletionItemKind::METHOD)
                && i.filter_text.as_deref().unwrap_or(&i.label) == name
        })
        .unwrap_or_else(|| panic!("no `{name}` method among {:?}", method_names(items)))
        .detail
        .clone()
        .unwrap_or_default()
}

fn consumer_running(body: &str) -> String {
    format!(
        "<?php\nnamespace App\\Models;\nclass Consumer {{\n    public function run(): void {{\n        {body}\n    }}\n}}\n"
    )
}

#[tokio::test]
async fn forwarded_builder_methods_show_the_models_builder_as_their_return_type() {
    let items = complete_in(
        &[],
        &[("src/Models/User.php", WHERE_USER_PHP)],
        &consumer_running("User::§"),
    )
    .await;
    assert_eq!(method_detail(&items, "where"), "Builder<User>");
    assert_eq!(method_detail(&items, "whereEmail"), "Builder<User>");
}

#[tokio::test]
async fn where_method_is_offered_for_the_implicit_primary_key() {
    let user = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass User extends Model {}\n";
    let items = complete_in(
        &[],
        &[("src/Models/User.php", user)],
        &consumer_running("User::§"),
    )
    .await;
    assert!(
        method_names(&items).contains(&"whereId"),
        "{:?}",
        method_names(&items)
    );
}

#[tokio::test]
async fn where_method_follows_a_custom_primary_key() {
    let user = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass User extends Model {\n    protected $primaryKey = 'passport_number';\n}\n";
    let items = complete_in(
        &[],
        &[("src/Models/User.php", user)],
        &consumer_running("User::§"),
    )
    .await;
    let methods = method_names(&items);
    assert!(methods.contains(&"wherePassportNumber"), "{methods:?}");
    assert!(!methods.contains(&"whereId"), "{methods:?}");
}

#[tokio::test]
#[ignore = "known gap: SoftDeletes does not contribute a deleted_at column"]
async fn soft_deletes_contributes_a_deleted_at_column() {
    let soft_deletes = "<?php\nnamespace Illuminate\\Database\\Eloquent;\ntrait SoftDeletes {\n    public static function withTrashed() {}\n}\n";
    let user = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nuse Illuminate\\Database\\Eloquent\\SoftDeletes;\nclass User extends Model {\n    use SoftDeletes;\n}\n";
    let extra = [
        ("vendor/illuminate/Eloquent/SoftDeletes.php", soft_deletes),
        ("src/Models/User.php", user),
    ];
    let statics = complete_in(&[], &extra, &consumer_running("User::§")).await;
    assert!(
        method_names(&statics).contains(&"whereDeletedAt"),
        "{:?}",
        method_names(&statics)
    );
    let members = complete_in(&[], &extra, &consumer_running("(new User())->§")).await;
    assert!(
        property_names(&members).contains(&"deleted_at"),
        "{:?}",
        property_names(&members)
    );
}

/// The label of the signature shown at the `§` in `consumer`.
async fn signature_label_in(
    replaced: &[(&str, &str)],
    extra: &[(&str, &str)],
    consumer: &str,
) -> String {
    use tower_lsp::LanguageServer;
    let (backend, _dir, uri, _text, position) = open_with(replaced, extra, consumer).await;
    let help = backend
        .signature_help(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        })
        .await
        .unwrap()
        .expect("expected signature help");
    help.signatures[help.active_signature.unwrap_or(0) as usize]
        .label
        .clone()
}

#[tokio::test]
async fn a_column_named_like_a_real_builder_method_does_not_shadow_it() {
    let query_builder = r#"<?php
namespace Illuminate\Database\Query;
class Builder {
    /** @return $this */
    public function whereDate($column, $operator, $value = null) { return $this; }
}
"#;
    let user = "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass User extends Model {\n    protected $fillable = ['date'];\n}\n";
    let replaced = [("vendor/illuminate/Query/Builder.php", query_builder)];
    let extra = [("src/Models/User.php", user)];
    for body in ["User::whereDate(§);", "User::query()->whereDate(§);"] {
        let label = signature_label_in(&replaced, &extra, &consumer_running(body)).await;
        assert!(label.contains("$operator"), "{body}: {label}");
    }
}

#[tokio::test]
async fn a_scope_named_like_a_column_where_method_wins() {
    let user = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Builder;
class User extends Model {
    protected $fillable = ['email'];
    public function scopeWhereEmail(Builder $query, string $domain): void {}
}
"#;
    let extra = [("src/Models/User.php", user)];
    let label = signature_label_in(&[], &extra, &consumer_running("User::whereEmail(§);")).await;
    assert!(label.contains("string $domain"), "{label}");

    let (backend, _dir, uri, text, _) =
        open_with(&[], &extra, &consumer_running("User::whereEmail§('x');")).await;
    let (line, character) = crate::common::line_char_of(&text, "whereEmail");
    let targets =
        definition_locations(goto_definition_at(&backend, &uri, line, character + 2).await);
    let (scope_line, _) = crate::common::line_char_of(user, "scopeWhereEmail");
    assert!(
        targets
            .iter()
            .any(|t| t.uri.path().ends_with("/User.php") && t.range.start.line == scope_line),
        "{targets:?}"
    );
}

#[tokio::test]
async fn a_models_own_static_method_wins_over_the_builder_method_of_that_name() {
    let model = MODEL_PHP.replace(
        "    protected $guarded = ['*'];",
        "    protected $guarded = ['*'];\n    /** @return \\Illuminate\\Database\\Eloquent\\Builder<static> */\n    public static function with($relations) { return new Builder(); }",
    );
    let builder = BUILDER_PHP.replace(
        "    /** @return static */\n    public function orderBy",
        "    /** @return $this */\n    public function with($relations, $callback = null) { return $this; }\n    /** @return static */\n    public function orderBy",
    );
    let replaced = [
        ("vendor/illuminate/Eloquent/Model.php", model.as_str()),
        ("vendor/illuminate/Eloquent/Builder.php", builder.as_str()),
    ];
    let extra = [("src/Models/User.php", WHERE_USER_PHP)];

    let label = signature_label_in(&replaced, &extra, &consumer_running("User::with(§);")).await;
    assert!(!label.contains("$callback"), "{label}");

    let items = complete_in(&replaced, &extra, &consumer_running("User::§")).await;
    assert_eq!(
        method_names(&items)
            .iter()
            .filter(|m| **m == "with")
            .count(),
        1,
        "{:?}",
        method_names(&items)
    );

    let (backend, _dir, uri, text, _) = open_with(
        &replaced,
        &extra,
        &consumer_running("User::with§('posts');"),
    )
    .await;
    let (line, character) = crate::common::line_char_of(&text, "with(");
    let targets =
        definition_locations(goto_definition_at(&backend, &uri, line, character + 1).await);
    assert!(
        targets.iter().all(|t| t.uri.path().ends_with("/Model.php")) && !targets.is_empty(),
        "{targets:?}"
    );
}

#[tokio::test]
#[ignore = "known gap: the base Model's own declared properties become where{Column} methods"]
async fn the_base_models_own_properties_are_not_columns() {
    let items = complete_in(
        &[],
        &[("src/Models/User.php", WHERE_USER_PHP)],
        &consumer_running("User::§"),
    )
    .await;
    let methods = method_names(&items);
    assert!(methods.contains(&"whereEmail"), "{methods:?}");
    assert!(!methods.contains(&"whereGuarded"), "{methods:?}");
}
