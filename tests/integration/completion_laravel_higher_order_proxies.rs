//! Integration tests for Laravel higher-order collection proxies.
//!
//! `$users->map->email` and `$users->filter->isActive()` go through
//! `HigherOrderCollectionProxy`, whose `__get` / `__call` run the proxied
//! collection method with a closure that performs the access on every item.

use crate::common::create_psr4_workspace;
use crate::common::{completion_response, open_php};
use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

// ─── Framework stubs ────────────────────────────────────────────────────────

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "Illuminate\\Support\\": "vendor/illuminate/Support/",
            "Illuminate\\Support\\Traits\\": "vendor/illuminate/Support/Traits/",
            "Illuminate\\Database\\Eloquent\\": "vendor/illuminate/Eloquent/"
        }
    }
}"#;

const ENUMERABLE_PHP: &str = r#"<?php
namespace Illuminate\Support;

/**
 * @template TKey of array-key
 * @template-covariant TValue
 */
interface Enumerable
{
}
"#;

/// The framework declares one `@property-read` per proxyable method on the
/// trait, not on `Collection` itself — subclasses only see them because a
/// parent's trait tags are inherited.
const ENUMERATES_VALUES_PHP: &str = r#"<?php
namespace Illuminate\Support\Traits;

use Illuminate\Support\HigherOrderCollectionProxy;

/**
 * @template TKey of array-key
 * @template-covariant TValue
 *
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $average
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $avg
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $contains
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $doesntContain
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $each
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $every
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $filter
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $first
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $flatMap
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $groupBy
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $keyBy
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $last
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $map
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $max
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $min
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $partition
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $percentage
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $reject
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $some
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $sortBy
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $sortByDesc
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $sum
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $unique
 */
trait EnumeratesValues
{
}
"#;

const PROXY_PHP: &str = r#"<?php
namespace Illuminate\Support;

/**
 * @template TKey of array-key
 * @template-covariant TValue
 *
 * @mixin \Illuminate\Support\Enumerable<TKey, TValue>
 * @mixin TValue
 */
class HigherOrderCollectionProxy
{
    public function __get($key) { return null; }
    public function __call($method, $parameters) { return null; }
}
"#;

const SUPPORT_COLLECTION_PHP: &str = r#"<?php
namespace Illuminate\Support;

use Illuminate\Support\Traits\EnumeratesValues;

/**
 * @template TKey of array-key
 * @template-covariant TValue
 *
 * @implements \Illuminate\Support\Enumerable<TKey, TValue>
 */
class Collection implements Enumerable
{
    /** @use \Illuminate\Support\Traits\EnumeratesValues<TKey, TValue> */
    use EnumeratesValues;

    /** @return int */
    public function count(): int { return 0; }

    /** @return string */
    public function implode(string $glue): string { return ''; }
}
"#;

const MODEL_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;

class Model
{
    public function save(): bool { return true; }
}
"#;

const ELOQUENT_COLLECTION_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;

use Illuminate\Support\Collection as BaseCollection;

/**
 * @template TKey of array-key
 * @template TModel of \Illuminate\Database\Eloquent\Model
 *
 * @extends \Illuminate\Support\Collection<TKey, TModel>
 */
class Collection extends BaseCollection
{
    /** @return $this */
    public function load($relations) { return $this; }
}
"#;

// ─── Application classes ────────────────────────────────────────────────────

const USER_PHP: &str = r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class User extends Model
{
    public string $email = '';
    public int $age = 0;
    public ?float $discount = null;

    /** @var array<string> */
    public array $tags = [];

    public function isActive(): bool { return true; }

    public function displayName(): string { return ''; }

    public function latestPost(): Post { return new Post(); }

    public function notify(): void {}
}
"#;

const POST_PHP: &str = r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Model;

class Post extends Model
{
    public string $title = '';
}
"#;

const USER_COLLECTION_PHP: &str = r#"<?php
namespace App;

use Illuminate\Database\Eloquent\Collection as EloquentCollection;

/**
 * @template TKey of array-key
 * @template TModel of \App\User
 *
 * @extends \Illuminate\Database\Eloquent\Collection<TKey, TModel>
 */
class UserCollection extends EloquentCollection
{
    public function verified(): static { return $this; }
}
"#;

// ─── Harness ────────────────────────────────────────────────────────────────

fn workspace() -> (Backend, tempfile::TempDir) {
    create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("vendor/illuminate/Support/Enumerable.php", ENUMERABLE_PHP),
            (
                "vendor/illuminate/Support/Traits/EnumeratesValues.php",
                ENUMERATES_VALUES_PHP,
            ),
            (
                "vendor/illuminate/Support/HigherOrderCollectionProxy.php",
                PROXY_PHP,
            ),
            (
                "vendor/illuminate/Support/Collection.php",
                SUPPORT_COLLECTION_PHP,
            ),
            ("vendor/illuminate/Eloquent/Model.php", MODEL_PHP),
            (
                "vendor/illuminate/Eloquent/Collection.php",
                ELOQUENT_COLLECTION_PHP,
            ),
            ("src/User.php", USER_PHP),
            ("src/Post.php", POST_PHP),
            ("src/UserCollection.php", USER_COLLECTION_PHP),
        ],
    )
}

/// Line (0-based) of the `$result` assignment produced by [`probe_source`].
const RESULT_LINE: u32 = 5;

/// Wrap `expression` in a probe file: the collection variable is declared
/// via `@var` so each test only has to spell the expression under test.
fn probe_source(collection_type: &str, expression: &str) -> String {
    format!(
        "<?php\nnamespace App;\n\n/** @var {} $items */\n$items = null;\n$result = {};\n",
        collection_type, expression
    )
}

/// The full hover markdown for `$result`, which includes the namespace of
/// the resolved class as well as its type.
fn hover_markdown(collection_type: &str, expression: &str) -> String {
    let (backend, dir) = workspace();
    let content = probe_source(collection_type, expression);
    let uri = format!("file://{}/src/Probe.php", dir.path().display());
    backend.update_ast(&uri, &content);

    let hover = backend
        .handle_hover(
            &uri,
            &content,
            Position {
                line: RESULT_LINE,
                character: 2,
            },
        )
        .expect("hover returned nothing");
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        _ => panic!("expected markup hover"),
    }
}

/// Resolve the type PHPantom infers for `$result = <expression>;`.
fn resolved_type(collection_type: &str, expression: &str) -> String {
    let markdown = hover_markdown(collection_type, expression);
    markdown
        .lines()
        .find_map(|l| l.strip_prefix("$result = "))
        .unwrap_or_else(|| panic!("hover did not report a type:\n{}", markdown))
        .to_string()
}

#[track_caller]
fn assert_type(collection_type: &str, expression: &str, expected: &str) {
    assert_eq!(
        resolved_type(collection_type, expression),
        expected,
        "for `{}` on `{}`",
        expression,
        collection_type
    );
}

const SUPPORT: &str = "\\Illuminate\\Support\\Collection<int, User>";
const ELOQUENT: &str = "\\Illuminate\\Database\\Eloquent\\Collection<int, User>";

// ─── The proxy object itself ────────────────────────────────────────────────

/// Reading a proxyable name off a collection yields the proxy, tagged with
/// the method it will run and the collection it came from.
#[test]
fn proxy_property_carries_method_and_collection() {
    assert_type(
        SUPPORT,
        "$items->map",
        "HigherOrderCollectionProxy<int, User, 'map', Collection>",
    );
}

/// The tag follows the receiver, which is what keeps a proxied `filter` on
/// the Eloquent subclass instead of degrading it to the base collection.
#[test]
fn proxy_property_records_the_receiving_subclass() {
    let markdown = hover_markdown(ELOQUENT, "$items->filter->isActive()");
    assert!(
        markdown.contains("namespace Illuminate\\Database\\Eloquent;"),
        "expected the Eloquent collection, got:\n{}",
        markdown
    );
    assert!(
        markdown.contains("$result = Collection<int, User>"),
        "unexpected type:\n{}",
        markdown
    );
}

// ─── map ────────────────────────────────────────────────────────────────────

#[test]
fn map_over_a_property_collects_that_property() {
    assert_type(SUPPORT, "$items->map->email", "Collection<int, string>");
}

#[test]
fn map_over_a_method_collects_its_return_type() {
    assert_type(
        SUPPORT,
        "$items->map->displayName()",
        "Collection<int, string>",
    );
}

#[test]
fn map_keeps_the_collection_key_type() {
    assert_type(
        "\\Illuminate\\Support\\Collection<string, User>",
        "$items->map->age",
        "Collection<string, int>",
    );
}

/// `Eloquent\Collection` is declared `@template TModel of Model`, so mapping
/// to a scalar has to fall back to the base collection — exactly what
/// `Eloquent\Collection::map()` does at runtime.
#[test]
fn map_to_a_scalar_degrades_an_eloquent_collection() {
    let markdown = hover_markdown(ELOQUENT, "$items->map->email");
    assert!(
        markdown.contains("namespace Illuminate\\Support;"),
        "expected the base Support collection, got:\n{}",
        markdown
    );
    assert!(
        markdown.contains("$result = Collection<int, string>"),
        "unexpected type:\n{}",
        markdown
    );
}

/// Mapping models to models keeps the Eloquent collection.
#[test]
fn map_to_a_model_keeps_an_eloquent_collection() {
    let markdown = hover_markdown(ELOQUENT, "$items->map->latestPost()");
    assert!(
        markdown.contains("namespace Illuminate\\Database\\Eloquent;"),
        "expected the Eloquent collection, got:\n{}",
        markdown
    );
    assert!(
        markdown.contains("$result = Collection<int, Post>"),
        "unexpected type:\n{}",
        markdown
    );
}

#[test]
fn flat_map_unwraps_the_member_element_type() {
    assert_type(
        SUPPORT,
        "$items->flatMap->tags",
        "Collection<array-key, string>",
    );
}

// ─── Methods that return the collection unchanged ───────────────────────────

#[test]
fn filtering_proxies_return_the_same_collection() {
    for expression in [
        "$items->filter->isActive()",
        "$items->reject->isActive()",
        "$items->each->notify()",
        "$items->unique->email",
        "$items->sortBy->age",
        "$items->sortByDesc->age",
    ] {
        assert_type(SUPPORT, expression, "Collection<int, User>");
    }
}

/// A project's own collection class is preserved the same way the framework
/// ones are.
#[test]
fn a_custom_collection_subclass_is_preserved() {
    assert_type(
        "\\App\\UserCollection<int, User>",
        "$items->filter->isActive()",
        "UserCollection<int, User>",
    );
}

// ─── Re-keying and grouping ─────────────────────────────────────────────────

#[test]
fn key_by_moves_the_member_into_the_key() {
    assert_type(
        SUPPORT,
        "$items->keyBy->email",
        "Collection<array-key, User>",
    );
}

#[test]
fn group_by_nests_the_collection() {
    assert_type(
        SUPPORT,
        "$items->groupBy->email",
        "Collection<array-key, Collection<int, User>>",
    );
}

#[test]
fn partition_nests_the_collection_under_integer_keys() {
    assert_type(
        SUPPORT,
        "$items->partition->isActive()",
        "Collection<int, Collection<int, User>>",
    );
}

// ─── Single values, predicates and aggregates ───────────────────────────────

#[test]
fn first_and_last_return_a_nullable_item() {
    assert_type(SUPPORT, "$items->first->isActive()", "?User");
    assert_type(SUPPORT, "$items->last->isActive()", "?User");
}

#[test]
fn predicates_return_bool() {
    for expression in [
        "$items->contains->isActive()",
        "$items->doesntContain->isActive()",
        "$items->every->isActive()",
        "$items->some->isActive()",
    ] {
        assert_type(SUPPORT, expression, "bool");
    }
}

#[test]
fn sum_takes_the_member_type_and_is_never_null() {
    assert_type(SUPPORT, "$items->sum->age", "int");
    // `sum` reduces from `0`, so totalling a nullable column cannot yield
    // `null` — reporting `?float` here would flag correct code that passes
    // the total to a `float` parameter.
    assert_type(SUPPORT, "$items->sum->discount", "float");
}

/// `min` / `max` reduce with no initial value, so an empty collection
/// yields `null` even for a non-nullable member.
#[test]
fn min_and_max_are_nullable() {
    assert_type(SUPPORT, "$items->min->age", "?int");
    assert_type(SUPPORT, "$items->max->age", "?int");
}

#[test]
fn averages_are_nullable_numbers() {
    assert_type(SUPPORT, "$items->avg->age", "int|float|null");
    assert_type(SUPPORT, "$items->average->age", "int|float|null");
}

#[test]
fn percentage_is_a_nullable_float() {
    assert_type(SUPPORT, "$items->percentage->isActive()", "?float");
}

// ─── Chaining ───────────────────────────────────────────────────────────────

/// The proxy result is an ordinary collection, so the chain continues.
#[test]
fn a_proxy_result_can_be_chained() {
    assert_type(SUPPORT, "$items->map->email->implode(', ')", "string");
    assert_type(SUPPORT, "$items->filter->isActive()->count()", "int");
}

/// The proxied member does not have to be declared on the item class
/// directly — an inherited one works the same way.
#[test]
fn inherited_members_are_proxied() {
    assert_type(SUPPORT, "$items->map->save()", "Collection<int, bool>");
}

// ─── Completion ─────────────────────────────────────────────────────────────

fn completion_labels(collection_type: &str, prefix_expression: &str) -> Vec<String> {
    let (backend, dir) = workspace();
    let content = format!(
        "<?php\nnamespace App;\n\n/** @var {} $items */\n$items = null;\n{}\n",
        collection_type, prefix_expression
    );
    let uri = format!("file://{}/src/Probe.php", dir.path().display());
    let url = Url::parse(&uri).unwrap();

    let response = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            open_php(&backend, &url, &content).await;
            completion_response(
                &backend,
                &url,
                RESULT_LINE,
                prefix_expression.len() as u32,
                None,
            )
            .await
        });

    match response {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        Some(CompletionResponse::List(list)) => list.items.into_iter().map(|i| i.label).collect(),
        None => Vec::new(),
    }
}

/// Completion after `$items->map->` offers the item type's members, so the
/// proxy is discoverable rather than something you have to already know.
#[test]
fn completion_after_a_proxy_offers_item_members() {
    let labels = completion_labels(SUPPORT, "$items->map->");
    for expected in ["email", "age", "isActive()", "displayName()"] {
        assert!(
            labels.iter().any(|l| l == expected),
            "expected `{}` in {:?}",
            expected,
            labels
        );
    }
}

// ─── Diagnostics ────────────────────────────────────────────────────────────

fn unknown_member_messages(expression: &str) -> Vec<String> {
    let (backend, dir) = workspace();
    let content = probe_source(SUPPORT, expression);
    let uri = format!("file://{}/src/Probe.php", dir.path().display());
    backend.update_ast(&uri, &content);

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(&uri, &content, &mut diagnostics);
    diagnostics
        .into_iter()
        .filter(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| matches!(c, NumberOrString::String(s) if s == "unknown_member"))
        })
        .map(|d| d.message)
        .collect()
}

/// A valid proxy access is not reported as an unknown member.
#[test]
fn a_valid_proxy_access_produces_no_diagnostic() {
    for expression in ["$items->map->email", "$items->filter->isActive()"] {
        let messages = unknown_member_messages(expression);
        assert!(
            messages.is_empty(),
            "unexpected diagnostics for `{}`: {:?}",
            expression,
            messages
        );
    }
}

/// A name that is not proxyable is still an unknown member on the
/// collection: tagging the real proxy properties does not invent new ones.
#[test]
fn a_name_that_is_not_a_proxy_is_still_reported() {
    let messages = unknown_member_messages("$items->notAProxy->email");
    assert!(
        messages.iter().any(|m| m.contains("notAProxy")),
        "expected an unknown-member diagnostic, got {:?}",
        messages
    );
}
