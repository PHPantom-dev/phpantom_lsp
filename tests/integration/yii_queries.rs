//! Yii queries: the tables and columns a `yiisoft/db` query names as
//! strings, read against the tables the project's migrations create.
use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, goto_definition_at, hover_on,
    message_text, open_document, position_of, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": {
        "yiisoft/active-record": "^1.1",
        "yiisoft/db": "^2.0",
        "yiisoft/db-migration": "^2.2"
    },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Yiisoft\\ActiveRecord\\": "vendor/yiisoft/active-record/src/",
        "Yiisoft\\Db\\": "vendor/yiisoft/db/src/"
    } },
    "extra": {
        "config-plugin-options": { "source-directory": "config" },
        "config-plugin": { "params": "params.php" }
    }
}"#;

/// The parts of `yiisoft/db` and `yiisoft/active-record` the tests reach,
/// declared as the packages declare them.
const PACKAGES: &[(&str, &str)] = &[
    (
        "vendor/yiisoft/db/src/Query/QueryInterface.php",
        r#"<?php
namespace Yiisoft\Db\Query;
interface QueryInterface
{
    public function from(array|string $tables): static;
    public function where(array|string|null $condition, array $params = []): static;
    public function andWhere(array|string $condition, array $params = []): static;
    public function orderBy(array|string $columns): static;
    public function select(array|string $columns, ?string $option = null): static;
    public function groupBy(array|string $columns): static;
    public function leftJoin(array|string $table, array|string $on = '', array $params = []): static;
    public function join(string $type, array|string $table, array|string $on = '', array $params = []): static;
    public function all(): array;
}
"#,
    ),
    (
        "vendor/yiisoft/db/src/Query/Query.php",
        r#"<?php
namespace Yiisoft\Db\Query;
class Query implements QueryInterface
{
    public function __construct(object $db) {}
    public function from(array|string $tables): static {}
    public function where(array|string|null $condition, array $params = []): static {}
    public function andWhere(array|string $condition, array $params = []): static {}
    public function orderBy(array|string $columns): static {}
    public function select(array|string $columns, ?string $option = null): static {}
    public function groupBy(array|string $columns): static {}
    public function leftJoin(array|string $table, array|string $on = '', array $params = []): static {}
    public function join(string $type, array|string $table, array|string $on = '', array $params = []): static {}
    public function all(): array {}
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/ActiveRecordInterface.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
interface ActiveRecordInterface
{
    public static function query(self|string|null $modelClass = null): ActiveQueryInterface;
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/AbstractActiveRecord.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
abstract class AbstractActiveRecord implements ActiveRecordInterface
{
    public static function query(ActiveRecordInterface|string|null $modelClass = null): ActiveQueryInterface
    {
        return new ActiveQuery($modelClass ?? static::class);
    }
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/ActiveRecord.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
class ActiveRecord extends AbstractActiveRecord
{
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/ActiveQueryInterface.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
use Yiisoft\Db\Query\QueryInterface;
interface ActiveQueryInterface extends QueryInterface
{
    public function joinWith(array|string $with): static;
    public function alias(string $alias): static;
    public function one(): array|ActiveRecordInterface|null;
}
"#,
    ),
    (
        "vendor/yiisoft/active-record/src/ActiveQuery.php",
        r#"<?php
namespace Yiisoft\ActiveRecord;
use Yiisoft\Db\Query\Query;
class ActiveQuery extends Query implements ActiveQueryInterface
{
    final public function __construct(ActiveRecordInterface|string $modelClass) {}
    public function joinWith(array|string $with): static {}
    public function alias(string $alias): static {}
    public function one(): array|ActiveRecordInterface|null {}
}
"#,
    ),
];

const PARAMS: &str = r#"<?php
return [
    'yiisoft/db-migration' => [
        'sourceNamespaces' => ['App\\Migration'],
    ],
];
"#;

const TABLE_PHP: &str = r#"<?php
namespace App\Db;

final class Table
{
    public const POST = '{{%post}}';
}
"#;

const MIGRATION: &str = r#"<?php
namespace App\Migration;

use App\Db\Table;
use Yiisoft\Db\Migration\MigrationBuilder;
use Yiisoft\Db\Migration\RevertibleMigrationInterface;

final class M251108000000CreateBlog implements RevertibleMigrationInterface
{
    public function up(MigrationBuilder $b): void
    {
        $column = $b->columnBuilder();
        $b->createTable(Table::POST, [
            'id' => $column::primaryKey(),
            'title' => $column::string()->notNull(),
            'status' => $column::integer()->notNull(),
            'author_id' => $column::integer()->notNull(),
            'publication_date' => $column::dateTime(),
        ]);
        $b->createTable('{{%user}}', [
            'id' => $column::primaryKey(),
            'name' => $column::string()->notNull(),
        ]);
    }

    public function down(MigrationBuilder $b): void
    {
        $b->dropTable('{{%user}}');
        $b->dropTable(Table::POST);
    }
}
"#;

const POST_PHP: &str = r#"<?php
namespace App\Blog;

use Yiisoft\ActiveRecord\ActiveRecord;

final class Post extends ActiveRecord
{
    public int $id;
    public string $title;
}
"#;

fn workspace() -> (Backend, tempfile::TempDir) {
    let mut files: Vec<(&str, &str)> = PACKAGES.to_vec();
    files.push(("config/params.php", PARAMS));
    files.push(("src/Db/Table.php", TABLE_PHP));
    files.push(("src/Migration/M251108000000CreateBlog.php", MIGRATION));
    files.push(("src/Blog/Post.php", POST_PHP));
    create_psr4_workspace(COMPOSER, &files)
}

async fn open(backend: &Backend, dir: &tempfile::TempDir, text: &str) -> Uri {
    let uri: Uri = crate::common::file_uri(&dir.path().join("src/Blog/demo.php"))
        .parse()
        .unwrap();
    open_document(backend, &uri, "php", text).await;
    uri
}

/// Where the definition at `needle` (plus `shift` characters) lands, as
/// the file name and line.
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

/// The messages of the diagnostics with `code` the file gets.
fn diagnostics(backend: &Backend, uri: &Uri, php: &str, code: &str) -> Vec<String> {
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), php, &mut out);
    out.iter()
        .filter(|d| matches!(&d.code, Some(Code::String(found)) if found == code))
        .map(|d| message_text(&d.message).to_string())
        .collect()
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

const MIGRATION_FILE: &str = "M251108000000CreateBlog.php";

#[tokio::test]
async fn tables_and_columns_go_to_the_migration_that_creates_them() {
    let php = r#"<?php
namespace App\Blog;

use App\Db\Table;
use Yiisoft\Db\Query\Query;

function demo(object $db): array
{
    return (new Query($db))
        ->from(Table::POST . ' p')
        ->leftJoin('{{%user}} u', 'u.id = p.author_id')
        ->where(['p.status' => 1])
        ->orderBy(['p.publication_date' => SORT_DESC])
        ->all();
}
"#;
    let (backend, dir) = workspace();
    let uri = open(&backend, &dir, php).await;
    let migration = |line: u32| vec![(MIGRATION_FILE.to_string(), line)];
    assert_eq!(
        definition_of(&backend, &uri, php, "user}} u", 0).await,
        migration(19)
    );
    assert_eq!(
        definition_of(&backend, &uri, php, "p.status", 2).await,
        migration(15)
    );
    assert_eq!(
        definition_of(&backend, &uri, php, "author_id'", 0).await,
        migration(16)
    );
    assert_eq!(
        definition_of(&backend, &uri, php, "u.id", 2).await,
        migration(20)
    );
    let hover = hover_on(&backend, &uri, php, "publication_date").await;
    assert!(
        hover.contains("Column `publication_date` of table `post`: `dateTime`"),
        "{hover}"
    );
    let hover = hover_on(&backend, &uri, php, "user}} u").await;
    assert!(hover.contains("Table `user`"), "{hover}");
}

#[tokio::test]
async fn tables_and_columns_no_migration_creates_are_reported() {
    let php = r#"<?php
namespace App\Blog;

use App\Db\Table;
use Yiisoft\Db\Query\Query;

final class Container
{
    public function where(array $condition): static {}
}

function demo(object $db, Query $query, Container $container): void
{
    (new Query($db))->from('psot')->all();
    (new Query($db))->from(['p' => Table::POST])->where(['p.titel' => 'x', 'p.title' => 'y'])->all();
    (new Query($db))->from('post p')->leftJoin('user u', ['u.id' => 'p.autor_id'])->where(['nme' => 'x'])->all();
    (new Query($db))->select('id, titel AS t')->from('post')->groupBy(['status'])->all();
    Post::query()->where(['and', ['titl' => 'x'], ['>', 'status', 1]])->one();
    Post::query()->alias('p')->andWhere('p.stauts = 1')->one();
    // Not checked: the FROM another statement sets, a qualifier the query
    // does not declare, a relation's joined table, and no query at all.
    $query->where(['anything' => 1])->all();
    (new Query($db))->from('post')->where(['x.anything' => 1])->all();
    Post::query()->joinWith('author')->where(['name' => 'x'])->one();
    $container->where(['anything' => 1]);
}
"#;
    let (backend, dir) = workspace();
    let uri = open(&backend, &dir, php).await;
    assert_eq!(
        diagnostics(&backend, &uri, php, "invalid_yii_table"),
        vec!["Table 'psot' is not created by any migration"]
    );
    assert_eq!(
        diagnostics(&backend, &uri, php, "invalid_yii_column"),
        vec![
            "Column 'titel' is not a column of table 'post'",
            "Column 'autor_id' is not a column of table 'post'",
            "Column 'nme' is not a column of any of the tables 'post', 'user'",
            "Column 'titel' is not a column of table 'post'",
            "Column 'titl' is not a column of table 'post'",
            "Column 'stauts' is not a column of table 'post'",
        ]
    );
}

#[tokio::test]
async fn tables_and_columns_complete() {
    let php = r#"<?php
namespace App\Blog;

use Yiisoft\Db\Query\Query;

function demo(object $db): void
{
    (new Query($db))->from('');
    (new Query($db))->from('post p')->leftJoin('user u', 'u.id = p.author_id')->where(['u.' => 1]);
    Post::query()->orderBy(['' => SORT_ASC]);
    Post::query()->select('id, ');
}
"#;
    let (backend, dir) = workspace();
    let uri = open(&backend, &dir, php).await;
    assert_eq!(
        complete(&backend, &uri, php, "from('").await,
        vec!["post", "user"]
    );
    assert_eq!(
        complete(&backend, &uri, php, "where(['u.").await,
        vec!["id", "name"]
    );
    assert_eq!(
        complete(&backend, &uri, php, "orderBy(['").await,
        vec!["author_id", "id", "publication_date", "status", "title"]
    );
    assert_eq!(
        complete(&backend, &uri, php, "select('id, ").await,
        vec!["author_id", "id", "publication_date", "status", "title"]
    );
}
