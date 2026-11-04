//! Property-selected collection types through the shared type engine.

use crate::common::{
    assert_assigned_types_on, complete_at, create_psr4_workspace, item_labels, split_cursor,
};
use phpantom_lsp::Backend;

const COLLECTION: &str = r#"<?php
namespace Illuminate\Support;
/**
 * @template TKey of array-key
 * @template TValue
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $keyBy
 * @property-read HigherOrderCollectionProxy<TKey, TValue> $groupBy
 */
class Collection {
    /**
     * @template TNewKey of array-key
     * @param (callable(TValue, TKey): TNewKey)|string $keyBy
     * @return static<($keyBy is string ? array-key : TNewKey), TValue>
     */
    public function keyBy($keyBy) {}
    /** @return static<array-key, static<TKey, TValue>> */
    public function groupBy($groupBy, $preserveKeys = false) {}
    /** @return static<array-key, mixed> */
    public function pluck($value, $key = null) {}
    /** @return TValue|null */
    public function first() {}
    /** @return array<TKey, TValue> */
    public function all(): array { return []; }
}
"#;

fn workspace() -> (Backend, tempfile::TempDir) {
    create_psr4_workspace(
        r#"{"require":{"laravel/framework":"^12.0"},"autoload":{"psr-4":{"App\\":"app/","Illuminate\\":"framework/"}}}"#,
        &[
            ("framework/Support/Collection.php", COLLECTION),
            (
                "framework/Support/LazyCollection.php",
                &COLLECTION.replace("class Collection", "class LazyCollection"),
            ),
            (
                "framework/Support/HigherOrderCollectionProxy.php",
                r#"<?php namespace Illuminate\Support;
/**
 * @template TKey
 * @template TValue
 */
class HigherOrderCollectionProxy {}"#,
            ),
            (
                "framework/Database/Eloquent/Model.php",
                "<?php namespace Illuminate\\Database\\Eloquent; class Model {}",
            ),
            (
                "framework/Database/Eloquent/Collection.php",
                r#"<?php namespace Illuminate\Database\Eloquent;
/**
 * @template TKey
 * @template TModel of Model
 * @extends \Illuminate\Support\Collection<TKey, TModel>
 */
class Collection extends \Illuminate\Support\Collection {}"#,
            ),
            (
                "app/User.php",
                r#"<?php namespace App;
class User extends \Illuminate\Database\Eloquent\Model {
    public int $id;
    public string $email;
    public bool $active;
    public ?int $parent_id;
    public float $score;
    public Profile $profile;
    /** @var list<string> */ public array $roles;
    /** @var array{tag: string, optional?: int} */ public array $data;
    /** @var Box<Profile> */ public Box $box;
    protected $casts = ['registered' => 'boolean'];
    public function getDisplayNameAttribute(): string { return ''; }
    public function label(): string { return ''; }
}
"#,
            ),
            (
                "app/Profile.php",
                "<?php namespace App; class Profile { public int $id; public string $name; }",
            ),
            (
                "app/Box.php",
                "<?php namespace App; /** @template T */ class Box { /** @var T */ public $item; }",
            ),
            (
                "app/Users.php",
                r#"<?php namespace App;
/** @extends \Illuminate\Database\Eloquent\Collection<int, User> */
class Users extends \Illuminate\Database\Eloquent\Collection {}"#,
            ),
            (
                "app/Collection.php",
                "<?php namespace App; class Collection { public function keyBy($key): int { return 0; } public function pluck($value): string { return ''; } }",
            ),
        ],
    )
}

fn assert_projection(collection: &str, statements: &str, expected: &[(&str, &str)]) {
    let (backend, dir) = workspace();
    let source = format!(
        "<?php\nnamespace Consumer;\nuse App\\User;\n/** @var {collection} $users */\n$users = null;\n{statements}\n"
    );
    let uri = format!("file://{}/app/Consumer.php", dir.path().display());
    backend.update_ast(&uri, &source);
    assert_assigned_types_on(&backend, &uri, &source, expected);
}

#[test]
fn literal_properties_determine_collection_keys_and_plucked_values() {
    for collection in [
        "\\Illuminate\\Support\\Collection<int, User>",
        "\\Illuminate\\Database\\Eloquent\\Collection<int, User>",
    ] {
        assert_projection(
            collection,
            r#"
$emails = $users->keyBy('email');
$ids = $users->keyBy('id');
$names = $users->pluck('display_name');
$indexed = $users->pluck(key: 'id', value: 'email');
$nested = $users->keyBy('profile.id');
$generic = $users->pluck('box.item.name');
$shape = $users->pluck('data.tag');
$unknown = $users->keyBy('missing');
$unknownValue = $users->pluck('missing');
$proxy = $users->keyBy->email;
$method = $users->keyBy->label();
$casts = $users->keyBy('registered');
$array = $users->keyBy('email')->all();
"#,
            &[
                ("$emails", "Collection<string, User>"),
                ("$ids", "Collection<int, User>"),
                ("$names", "Collection<int, string>"),
                ("$indexed", "Collection<int, string>"),
                ("$nested", "Collection<int, User>"),
                ("$generic", "Collection<int, string>"),
                ("$shape", "Collection<int, string>"),
                ("$unknown", "Collection<array-key, User>"),
                ("$unknownValue", "Collection<int, mixed>"),
                ("$proxy", "Collection<string, User>"),
                ("$method", "Collection<string, User>"),
                ("$casts", "Collection<int, User>"),
                ("$array", "array<string, User>"),
            ],
        );
    }
}

#[test]
fn grouping_adds_one_level_per_property_and_preserves_only_item_keys() {
    assert_projection(
        "\\Illuminate\\Support\\Collection<string, User>",
        r#"
$groups = $users->groupBy('id');
$preserved = $users->groupBy(preserveKeys: true, groupBy: 'id');
$nested = $users->groupBy(['active', 'email'], true);
$legacyArray = $users->groupBy(array('active', 'email'));
$explicitReset = $users->groupBy('id', FALSE);
$explicitPreserve = $users->groupBy('id', TRUE);
$proxy = $users->groupBy->id;
$roles = $users->groupBy('roles');
$nullable = $users->pluck('data.optional');
"#,
        &[
            ("$groups", "Collection<int, Collection<int, User>>"),
            ("$preserved", "Collection<int, Collection<string, User>>"),
            (
                "$nested",
                "Collection<int, Collection<string, Collection<string, User>>>",
            ),
            (
                "$legacyArray",
                "Collection<int, Collection<string, Collection<int, User>>>",
            ),
            ("$explicitReset", "Collection<int, Collection<int, User>>"),
            (
                "$explicitPreserve",
                "Collection<int, Collection<string, User>>",
            ),
            ("$proxy", "Collection<int, Collection<int, User>>"),
            ("$roles", "Collection<string, Collection<int, User>>"),
            ("$nullable", "Collection<int, ?int>"),
        ],
    );
}

#[test]
fn collection_subclasses_and_unrelated_names_keep_their_meaning() {
    assert_projection(
        "\\App\\Users",
        r#"
$keyed = $users->keyBy('email');
$plucked = $users->pluck('email');
$groups = $users->groupBy('id');
$callback = $users->keyBy(fn (User $user): int => $user->id);
$other = (new \App\Collection())->keyBy('email');
"#,
        &[
            ("$keyed", "Users<string, User>"),
            ("$plucked", "Collection<int, string>"),
            ("$groups", "Users<int, Users<int, User>>"),
            ("$callback", "Users<int, User>"),
            ("$other", "int"),
        ],
    );
}

#[test]
fn projection_names_ignore_case_and_callbacks_keep_template_inference() {
    assert_projection(
        "\\Illuminate\\Support\\Collection<int, User>",
        r#"
$keyed = $users->KeYbY('email');
$grouped = $users->GrOuPbY('email');
$plucked = $users->PLUCK('profile');
$callback = $users->KeYbY(fn (User $user): string => $user->email);
"#,
        &[
            ("$keyed", "Collection<string, User>"),
            ("$grouped", "Collection<string, Collection<int, User>>"),
            ("$plucked", "Collection<int, Profile>"),
            ("$callback", "Collection<string, User>"),
        ],
    );
}

#[test]
fn collection_first_class_callables_infer_the_invocation_arguments() {
    assert_projection(
        "\\Illuminate\\Support\\Collection<string, User>",
        r#"
$group = $users->groupBy(...);
$groups = $group('email');
"#,
        &[("$groups", "Collection<string, Collection<int, User>>")],
    );
}

#[test]
fn lazy_collection_projection_keeps_the_outer_collection_lazy() {
    assert_projection(
        "\\Illuminate\\Support\\LazyCollection<string, User>",
        r#"
$keyed = $users->keyBy('email');
$plucked = $users->pluck('email');
$groups = $users->groupBy('id', true);
$proxy = $users->groupBy->id;
"#,
        &[
            ("$keyed", "LazyCollection<string, User>"),
            ("$plucked", "LazyCollection<int, string>"),
            ("$groups", "LazyCollection<int, Collection<string, User>>"),
            ("$proxy", "LazyCollection<int, Collection<int, User>>"),
        ],
    );
}

#[test]
fn rekeyed_custom_collections_keep_the_new_keys_through_grouping() {
    assert_projection(
        "\\App\\Users",
        r#"
$groups = $users->keyBy('email')->groupBy('id', true);
$nested = $users->keyBy('email')->groupBy(['active', 'email'], true);
"#,
        &[
            ("$groups", "Users<int, Users<string, User>>"),
            (
                "$nested",
                "Collection<int, Users<string, Users<string, User>>>",
            ),
        ],
    );
}

#[test]
fn projections_handle_nullable_union_and_array_items() {
    assert_projection(
        "\\Illuminate\\Support\\Collection<string, array{owner: ?\\App\\User, code: int|string}>",
        r#"
$names = $users->pluck('owner.email');
$keyed = $users->keyBy('code');
$identity = $users->pluck(null, null);
"#,
        &[
            ("$names", "Collection<int, ?string>"),
            (
                "$keyed",
                "Collection<int|string, array{owner: ?User, code: int|string}>",
            ),
            (
                "$identity",
                "Collection<int, array{owner: ?User, code: int|string}>",
            ),
        ],
    );
    assert_projection(
        "\\Illuminate\\Support\\Collection<int, array<string, string>>",
        "$values = $users->pluck('name');",
        &[("$values", "Collection<int, ?string>")],
    );
    assert_projection(
        "\\Illuminate\\Support\\Collection<int, \\App\\User|\\App\\Profile>",
        "$ids = $users->pluck('id');",
        &[("$ids", "Collection<int, int>")],
    );
}

#[test]
fn unknown_selectors_keep_declarations_and_unknown_preservation_keeps_both_keys() {
    assert_projection(
        "\\Illuminate\\Support\\Collection<string, User>",
        r#"
/** @var bool $preserve */
$preserve = null;
/** @var string $selector */
$selector = null;
$groups = $users->groupBy('id', $preserve);
$unknown = $users->keyBy($selector);
$noGroups = $users->groupBy([]);
$callbackGroup = $users->groupBy([fn (User $user) => 'email']);
$keyedGroup = $users->groupBy(['by' => 'email']);
$identity = $users->pluck(null);
"#,
        &[
            ("$groups", "Collection<int, Collection<string|int, User>>"),
            ("$unknown", "Collection<array-key, User>"),
            (
                "$noGroups",
                "Collection<array-key, Collection<string, User>>",
            ),
            (
                "$callbackGroup",
                "Collection<array-key, Collection<string, User>>",
            ),
            ("$keyedGroup", "Collection<string, Collection<int, User>>"),
            ("$identity", "Collection<int, User>"),
        ],
    );
}

#[tokio::test]
async fn plucked_objects_complete_on_a_direct_method_chain() {
    let (backend, dir) = workspace();
    for collection in [
        r"\Illuminate\Support\Collection<int, \App\User>",
        r"\Illuminate\Support\Collection<int, \App\User>|\Illuminate\Support\LazyCollection<int, \App\User>",
    ] {
        let source = r#"<?php
namespace Consumer;
/** @param COLLECTION $users */
function show($users): void {
    $users->pluck('profile')->first()->§
}
"#
        .replace("COLLECTION", collection);
        let (content, position) = split_cursor(&source);
        let uri = tower_lsp_server::gen_lsp_types::Uri::from_file_path(
            dir.path().join("app/Consumer.php"),
        )
        .unwrap();
        let labels = item_labels(
            complete_at(&backend, &uri, &content, position.line, position.character).await,
        );
        assert!(labels.iter().any(|label| label == "id"), "{labels:?}");
        assert!(labels.iter().any(|label| label == "name"), "{labels:?}");
    }
}
