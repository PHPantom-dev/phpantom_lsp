//! Yii Active Record: the record a query is for, and the attributes and
//! relations a record and its queries name as strings.
use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, edits_for_uri,
    goto_definition_at, hover_on, message_text, open_document, position_of, rename,
    response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "yiisoft/active-record": "^1.1" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\ActiveRecord\\": "vendor/yiisoft/active-record/src/"
    } }
}"#;

/// The parts of `yiisoft/active-record` the tests reach, declared as the
/// package declares them.
const PACKAGE: &[(&str, &str)] = &[
    (
        "vendor/yiisoft/active-record/src/ActiveRecordInterface.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
interface ActiveRecordInterface
{
    public static function query(self|string|null $modelClass = null): ActiveQueryInterface;
    public function createQuery(self|string|null $modelClass = null): ActiveQueryInterface;
    public function get(string $propertyName): mixed;
    public function set(string $propertyName, mixed $value): void;
    public function oldValue(string $propertyName): mixed;
    public function relation(string $name): self|array|null;
    public function relationQuery(string $name): ActiveQueryInterface;
    public function isRelationPopulated(string $name): bool;
    public function hasOne(self|string $modelClass, array $link): ActiveQueryInterface;
    public function hasMany(self|string $modelClass, array $link): ActiveQueryInterface;
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/AbstractActiveRecord.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
abstract class AbstractActiveRecord implements ActiveRecordInterface
{
    private array $related = [];
    public static function query(ActiveRecordInterface|string|null $modelClass = null): ActiveQueryInterface
    {
        return new ActiveQuery($modelClass ?? static::class);
    }
    public function createQuery(ActiveRecordInterface|string|null $modelClass = null): ActiveQueryInterface {}
    public function get(string $propertyName): mixed {}
    public function set(string $propertyName, mixed $value): void {}
    public function oldValue(string $propertyName): mixed {}
    public function relation(string $name): ActiveRecordInterface|array|null {}
    public function relationQuery(string $name): ActiveQueryInterface {}
    public function isRelationPopulated(string $name): bool {}
    public function hasOne(ActiveRecordInterface|string $modelClass, array $link): ActiveQueryInterface {}
    public function hasMany(ActiveRecordInterface|string $modelClass, array $link): ActiveQueryInterface {}
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/ActiveRecord.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
class ActiveRecord extends AbstractActiveRecord
{
    public function get(string $propertyName): mixed {}
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/ActiveQueryInterface.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
interface ActiveQueryInterface
{
    public function with(array|string ...$with): static;
    public function joinWith(array|string $with, array|bool $eagerLoading = true, array|string $joinType = 'LEFT JOIN'): static;
    public function where(array|string $condition, array $params = []): static;
    public function one(): array|ActiveRecordInterface|null;
    /** @psalm-return array<ActiveRecordInterface|array> */
    public function all(): array;
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/ActiveQuery.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
class ActiveQuery implements ActiveQueryInterface
{
    final public function __construct(ActiveRecordInterface|string $modelClass) {}
    public function with(array|string ...$with): static {}
    public function joinWith(array|string $with, array|bool $eagerLoading = true, array|string $joinType = 'LEFT JOIN'): static {}
    public function where(array|string $condition, array $params = []): static {}
    public function one(): array|ActiveRecordInterface|null {}
    public function all(): array {}
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/Trait/MagicRelationsTrait.php",
        r#"<?php
namespace Yiisoft\ActiveRecord\Trait;
use Yiisoft\ActiveRecord\ActiveQueryInterface;
trait MagicRelationsTrait
{
    public function relationQuery(string $name): ActiveQueryInterface {}
}
"#,
    ),
];

const USER_PHP: &str = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveQueryInterface;
use Yiisoft\ActiveRecord\ActiveRecord;

final class User extends ActiveRecord
{
    public int $id;
    public string $name;

    public function relationQuery(string $name): ActiveQueryInterface
    {
        return match ($name) {
            'posts' => $this->hasMany(Post::class, ['author_id' => 'id']),
            'profile' => $this->hasOne(Profile::class, ['user_id' => 'id']),
            default => parent::relationQuery($name),
        };
    }
}
"#;

const POST_PHP: &str = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveQueryInterface;
use Yiisoft\ActiveRecord\ActiveRecord;
use Yiisoft\ActiveRecord\Trait\MagicRelationsTrait;

final class Post extends ActiveRecord
{
    use MagicRelationsTrait;

    public int $id;
    public string $title;
    private int $author_id;

    public function getAuthorQuery(): ActiveQueryInterface
    {
        return $this->hasOne(User::class, ['id' => 'author_id']);
    }

    public function getTitle(): string
    {
        return $this->get('title');
    }
}
"#;

const PROFILE_PHP: &str = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveRecord;

/**
 * A record whose attributes are the table's columns.
 */
#[\AllowDynamicProperties]
final class Profile extends ActiveRecord
{
}
"#;

fn workspace(extra: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let mut files: Vec<(&str, &str)> = PACKAGE.to_vec();
    files.push(("src/Blog/User.php", USER_PHP));
    files.push(("src/Blog/Post.php", POST_PHP));
    files.push(("src/Blog/Profile.php", PROFILE_PHP));
    files.extend_from_slice(extra);
    create_psr4_workspace(COMPOSER, &files)
}

async fn open(backend: &Backend, dir: &tempfile::TempDir, path: &str, text: &str) -> Uri {
    let uri: Uri = crate::common::file_uri(&dir.path().join(path))
        .parse()
        .unwrap();
    open_document(backend, &uri, "php", text).await;
    uri
}

fn types(hover: &str) -> &str {
    hover
        .lines()
        .find_map(|line| line.split_once(" = ").map(|(_, ty)| ty))
        .unwrap_or(hover)
}

#[tokio::test]
async fn a_query_returns_the_record_it_is_built_for() {
    let php = r#"<?php
namespace App\Blog;

function demo(): void
{
    $post = Post::query()->with('author')->where(['id' => 1])->one();
    $post;
    $posts = Post::query()->all();
    $posts;
    $author = (new Post())->getAuthorQuery()->one();
    $author;
    $profile = (new User())->hasOne(Profile::class, [])->one();
    $profile;
}
"#;
    let (backend, dir) = workspace(&[]);
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    let hover = |needle: &'static str| {
        let backend = &backend;
        let uri = &uri;
        async move { hover_on(backend, uri, php, needle).await }
    };
    assert_eq!(
        types(&hover("$post;").await),
        "Post|array<string, mixed>|null"
    );
    assert_eq!(
        types(&hover("$posts;").await),
        "array<Post|array<string, mixed>>"
    );
    // A getter declared as a bare query returns one for the class its
    // body builds it for.
    assert_eq!(
        types(&hover("$author;").await),
        "User|array<string, mixed>|null"
    );
    assert_eq!(
        types(&hover("$profile;").await),
        "Profile|array<string, mixed>|null"
    );
}

/// Where the definition at `needle` (plus `shift` characters) in the open
/// document lands, as the file name and line.
async fn definition_of(
    backend: &Backend,
    uri: &Uri,
    php: &str,
    needle: &str,
    shift: u32,
) -> Vec<(String, u32)> {
    let position = position_of(php, needle);
    let response =
        goto_definition_at(backend, uri, position.line, position.character + shift).await;
    definition_locations(response)
        .into_iter()
        .map(|location| {
            let path = location.uri.path().to_string();
            let file = path.rsplit('/').next().unwrap_or_default().to_string();
            (file, location.range.start.line)
        })
        .collect()
}

#[tokio::test]
async fn an_attribute_name_goes_to_the_property() {
    let (backend, dir) = workspace(&[]);
    let uri = open(&backend, &dir, "src/Blog/Post.php", POST_PHP).await;
    assert_eq!(
        definition_of(&backend, &uri, POST_PHP, "'title')", 2).await,
        vec![("Post.php".to_string(), 12)]
    );
}

#[tokio::test]
async fn renaming_a_property_renames_the_attribute_names() {
    let php = r#"<?php
namespace App\Blog;

function demo(Post $post): void
{
    $post->set('title', 'Hello');
}
"#;
    let (backend, dir) = workspace(&[]);
    let post_uri = open(&backend, &dir, "src/Blog/Post.php", POST_PHP).await;
    let demo_uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    let position = position_of(POST_PHP, "$title;");
    let edit = rename(
        &backend,
        &post_uri,
        position.line,
        position.character + 1,
        "heading",
    )
    .await
    .expect("rename edits");
    let in_post = edits_for_uri(&edit, &post_uri);
    assert_eq!(in_post.len(), 2, "{in_post:?}");
    let in_demo = edits_for_uri(&edit, &demo_uri);
    assert_eq!(in_demo.len(), 1, "{in_demo:?}");
    assert_eq!(in_demo[0].new_text, "heading");
}

#[tokio::test]
async fn a_relation_name_goes_to_its_declaration() {
    let php = r#"<?php
namespace App\Blog;

function demo(User $user): void
{
    Post::query()->with('author')->one();
    $user->relation('posts');
    Post::query()->with(['author.profile' => static fn ($query) => $query])->all();
}
"#;
    let (backend, dir) = workspace(&[]);
    open(&backend, &dir, "src/Blog/User.php", USER_PHP).await;
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    // The getter, under `MagicRelationsTrait`.
    assert_eq!(
        definition_of(&backend, &uri, php, "'author')", 2).await,
        vec![("Post.php".to_string(), 15)]
    );
    // An arm of `relationQuery()`.
    assert_eq!(
        definition_of(&backend, &uri, php, "'posts'", 2).await,
        vec![("User.php".to_string(), 14)]
    );
    // A path continues from the record the relation before it is for.
    assert_eq!(
        definition_of(&backend, &uri, php, "profile'", 1).await,
        vec![("User.php".to_string(), 15)]
    );
    assert_eq!(
        definition_of(&backend, &uri, php, "'author.profile'", 2).await,
        vec![("Post.php".to_string(), 15)]
    );
}

/// The messages of the diagnostics with `code` the file gets.
fn diagnostics(backend: &Backend, uri: &Uri, php: &str, code: &str) -> Vec<String> {
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), php, &mut out);
    out.iter()
        .filter(|d| matches!(&d.code, Some(Code::String(found)) if found == code))
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn an_attribute_the_record_does_not_have_is_reported() {
    let php = r#"<?php
namespace App\Blog;

final class Container
{
    public function get(string $id): mixed {}
}

function demo(Post $post, Profile $profile, Container $container): void
{
    $post->get('title');
    $post->set('titel', 'Hello');
    $profile->get('bio');
    $container->get('logger');
}
"#;
    let (backend, dir) = workspace(&[]);
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert_eq!(
        diagnostics(&backend, &uri, php, "invalid_yii_attribute"),
        vec!["Attribute 'titel' is not a property of record App\\Blog\\Post"]
    );
    assert!(
        diagnostics(&backend, &uri, php, "unknown_member").is_empty(),
        "{:?}",
        diagnostics(&backend, &uri, php, "unknown_member")
    );
}

#[tokio::test]
async fn a_relation_the_record_does_not_declare_is_reported() {
    let php = r#"<?php
namespace App\Blog;

function demo(User $user): void
{
    Post::query()->with('author', 'autor')->one();
    Post::query()->joinWith(['author.profile', 'author.pofile'])->all();
    $user->relation('posts');
    $user->relation('post');
    $user->relationQuery('profile');
}
"#;
    let (backend, dir) = workspace(&[]);
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert_eq!(
        diagnostics(&backend, &uri, php, "invalid_yii_relation"),
        vec![
            "Relation 'autor' is not declared by record App\\Blog\\Post",
            "Relation 'pofile' is not declared by record App\\Blog\\User",
            "Relation 'post' is not declared by record App\\Blog\\User",
        ]
    );
}

#[tokio::test]
async fn relations_a_record_cannot_list_are_not_reported() {
    let open_record = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveQueryInterface;
use Yiisoft\ActiveRecord\ActiveRecord;

final class Tag extends ActiveRecord
{
    public function relationQuery(string $name): ActiveQueryInterface
    {
        return $this->hasMany(Post::class, ['tag' => $name]);
    }
}
"#;
    let php = r#"<?php
namespace App\Blog;

function demo(Tag $tag): void
{
    $tag->relation('anything');
}
"#;
    let (backend, dir) = workspace(&[("src/Blog/Tag.php", open_record)]);
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert!(diagnostics(&backend, &uri, php, "invalid_yii_relation").is_empty());
}

async fn complete(backend: &Backend, uri: &Uri, php: &str, after: &str) -> Vec<String> {
    let offset = php.find(after).expect("marker") + after.len();
    let position = crate::common::offset_position(php, offset);
    let mut labels = response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    );
    labels.sort();
    labels
}

#[tokio::test]
async fn attribute_and_relation_names_complete() {
    let php = r#"<?php
namespace App\Blog;

function demo(Post $post, User $user): void
{
    $post->get('');
    $user->relation('');
    Post::query()->with('');
    Post::query()->with(['author.']);
}
"#;
    let (backend, dir) = workspace(&[]);
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert_eq!(
        complete(&backend, &uri, php, "get('").await,
        vec!["author_id", "id", "title"]
    );
    assert_eq!(
        complete(&backend, &uri, php, "relation('").await,
        vec!["posts", "profile"]
    );
    assert_eq!(
        complete(&backend, &uri, php, "with('").await,
        vec!["author"]
    );
    assert_eq!(
        complete(&backend, &uri, php, "with(['author.").await,
        vec!["posts", "profile"]
    );
}

#[tokio::test]
async fn relations_a_trait_declares_count() {
    let tagged = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveQueryInterface;

trait Tagged
{
    public function getTagsQuery(): ActiveQueryInterface
    {
        return $this->hasMany(Tag::class, ['post_id' => 'id']);
    }
}
"#;
    let tag = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveRecord;
use Yiisoft\ActiveRecord\Trait\MagicRelationsTrait;

final class Tag extends ActiveRecord
{
    use MagicRelationsTrait;
    use Tagged;

    public string $label;
}
"#;
    let php = r#"<?php
namespace App\Blog;

/** @template T */
final class Collection
{
    public function with(string ...$names): static {}
}

/** @param Collection<Tag> $tags */
function demo(Tag $tag, Collection $tags): void
{
    $tag->relation('tags');
    $tag->relation('tagz');
    $tags->with('anything');
}
"#;
    let (backend, dir) = workspace(&[("src/Blog/Tagged.php", tagged), ("src/Blog/Tag.php", tag)]);
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert_eq!(
        diagnostics(&backend, &uri, php, "invalid_yii_relation"),
        vec!["Relation 'tagz' is not declared by record App\\Blog\\Tag"]
    );
    assert_eq!(
        definition_of(&backend, &uri, php, "'tags'", 2).await,
        vec![("Tagged.php".to_string(), 7)]
    );
}

/// A project whose `yiisoft/config` params point the migrator at the
/// migrations, with `migrations` in the params' `yiisoft/db-migration`.
fn migrated_workspace(migrations: &str, extra: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let composer = r#"{
    "require": { "yiisoft/active-record": "^1.1", "yiisoft/db-migration": "^2.2" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\ActiveRecord\\": "vendor/yiisoft/active-record/src/"
    } },
    "extra": {
        "config-plugin-options": { "source-directory": "config" },
        "config-plugin": { "params": "params.php" }
    }
}"#;
    let params =
        format!("<?php\nreturn [\n    'yiisoft/db-migration' => [\n{migrations}\n    ],\n];\n");
    let mut files: Vec<(&str, &str)> = PACKAGE.to_vec();
    files.push(("src/Blog/User.php", USER_PHP));
    files.push(("src/Blog/Post.php", POST_PHP));
    files.push(("src/Blog/Profile.php", PROFILE_PHP));
    files.push(("config/params.php", &params));
    files.extend_from_slice(extra);
    create_psr4_workspace(composer, &files)
}

const CREATE_PROFILE: &str = r#"<?php
namespace App\Migration;

use Yiisoft\Db\Migration\MigrationBuilder;
use Yiisoft\Db\Migration\RevertibleMigrationInterface;

final class M251108000000CreateProfile implements RevertibleMigrationInterface
{
    public function up(MigrationBuilder $b): void
    {
        $column = $b->columnBuilder();
        $b->createTable('{{%profile}}', [
            'id' => $column::primaryKey(),
            'user_id' => $column::integer()->notNull(),
            'bio' => $column::text(),
        ]);
    }

    public function down(MigrationBuilder $b): void
    {
        $b->dropTable('{{%profile}}');
    }
}
"#;

const RENAME_BIO: &str = r#"<?php
namespace App\Migration;

use Yiisoft\Db\Migration\MigrationBuilder;
use Yiisoft\Db\Migration\RevertibleMigrationInterface;

final class M251109000000RenameBio implements RevertibleMigrationInterface
{
    public function up(MigrationBuilder $b): void
    {
        $b->renameColumn('{{%profile}}', 'bio', 'about');
        $b->addColumn('{{%profile}}', 'website', 'string');
    }

    public function down(MigrationBuilder $b): void
    {
    }
}
"#;

const BY_NAMESPACE: &str = "        'sourceNamespaces' => ['App\\\\Migration'],";

#[tokio::test]
async fn columns_of_a_record_without_properties_come_from_the_migrations() {
    let php = r#"<?php
namespace App\Blog;

function demo(Profile $profile): void
{
    $profile->get('');
    $profile->get('about');
    $profile->set('bio', 'Hello');
    $profile->get('website');
}
"#;
    let (backend, dir) = migrated_workspace(
        BY_NAMESPACE,
        &[
            (
                "src/Migration/M251108000000CreateProfile.php",
                CREATE_PROFILE,
            ),
            ("src/Migration/M251109000000RenameBio.php", RENAME_BIO),
        ],
    );
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert_eq!(
        complete(&backend, &uri, php, "get('").await,
        vec!["about", "id", "user_id", "website"]
    );
    // A renamed column goes to the migration that renames it.
    assert_eq!(
        definition_of(&backend, &uri, php, "'about'", 2).await,
        vec![("M251109000000RenameBio.php".to_string(), 10)]
    );
    assert_eq!(
        definition_of(&backend, &uri, php, "'website'", 2).await,
        vec![("M251109000000RenameBio.php".to_string(), 11)]
    );
    assert_eq!(
        diagnostics(&backend, &uri, php, "invalid_yii_attribute"),
        vec!["Attribute 'bio' is not a column of table 'profile' of record App\\Blog\\Profile"]
    );
    let hover = hover_on(&backend, &uri, php, "'about'").await;
    assert!(
        hover.contains("Column `about` of table `profile`: `text`"),
        "{hover}"
    );
}

#[tokio::test]
async fn a_record_names_its_table_and_migrations_are_found_by_path() {
    let record = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveRecord;

final class Bio extends ActiveRecord
{
    public function tableName(): string
    {
        return '{{%profile}}';
    }
}
"#;
    let php = r#"<?php
namespace App\Blog;

function demo(Bio $bio, Profile $profile): void
{
    $bio->get('user_id');
    $bio->get('usr_id');
}
"#;
    let by_path = "        'sourcePaths' => [dirname(__DIR__) . '/migrations'],";
    let (backend, dir) = migrated_workspace(
        by_path,
        &[
            ("migrations/M251108000000CreateProfile.php", CREATE_PROFILE),
            ("src/Blog/Bio.php", record),
        ],
    );
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert_eq!(
        diagnostics(&backend, &uri, php, "invalid_yii_attribute"),
        vec!["Attribute 'usr_id' is not a column of table 'profile' of record App\\Blog\\Bio"]
    );
    assert_eq!(
        definition_of(&backend, &uri, php, "'user_id'", 2).await,
        vec![("M251108000000CreateProfile.php".to_string(), 13)]
    );
}

#[tokio::test]
async fn columns_a_migration_cannot_list_are_not_reported() {
    let open_table = r#"<?php
namespace App\Migration;

use Yiisoft\Db\Migration\MigrationBuilder;

final class M251109000000AddColumns
{
    public function up(MigrationBuilder $b): void
    {
        foreach (['twitter', 'github'] as $name) {
            $b->addColumn('{{%profile}}', $name, 'string');
        }
        $b->addColumn('{{%profile}}', $this->column(), 'string');
    }
}
"#;
    let php = r#"<?php
namespace App\Blog;

function demo(Profile $profile): void
{
    $profile->get('twitter');
}
"#;
    let (backend, dir) = migrated_workspace(
        BY_NAMESPACE,
        &[
            (
                "src/Migration/M251108000000CreateProfile.php",
                CREATE_PROFILE,
            ),
            ("src/Migration/M251109000000AddColumns.php", open_table),
        ],
    );
    let uri = open(&backend, &dir, "src/Blog/demo.php", php).await;
    assert!(diagnostics(&backend, &uri, php, "invalid_yii_attribute").is_empty());
    assert_eq!(
        complete(&backend, &uri, php, "get('").await,
        vec!["bio", "id", "user_id"]
    );
}
