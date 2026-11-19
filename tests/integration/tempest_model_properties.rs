//! Tempest property names follow the model carried by the receiver type.

use crate::common::{
    complete_at_opened, complete_labels_at_opened, create_psr4_workspace, definition_locations,
    definitions_at, goto_definition_at, hover_on, open_php, position_after, position_of,
    workspace_uri,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const FRAMEWORK: &str = r"<?php
namespace Tempest\Database\Builder\QueryBuilders;
/** @template TModel */
class QueryBuilder {
    /** @return SelectQueryBuilder<TModel> */
    public function select(string ...$columns): SelectQueryBuilder {}
    /** @return TModel */
    public function create(mixed ...$params) {}
    /** @return TModel */
    public function new(mixed ...$params) {}
    /** @return SelectQueryBuilder<TModel> */
    public function find(mixed ...$conditions): SelectQueryBuilder {}
    /** @return TModel */
    public function findOrNew(array $find, array $update) {}
    /** @return TModel */
    public function updateOrCreate(array $find, array $update) {}
    /** @return CountQueryBuilder<TModel> */
    public function count(?string $column = null): CountQueryBuilder {}
    /** @return UpdateQueryBuilder<TModel> */
    public function update(mixed ...$values): UpdateQueryBuilder {}
    /** @return DeleteQueryBuilder<TModel> */
    public function delete(): DeleteQueryBuilder {}
    public function insert(mixed ...$values) {}
}
/** @template TModel */
trait HasWhereQueryBuilderMethods {
    use HasConvenientWhereMethods;
    public function where(string $statement, mixed ...$bindings): self {}
    public function whereField(string $field, mixed $value, string $operator = '='): self {}
    public function whereRaw(string $statement, mixed ...$bindings): self {}
    public function whereGroup(callable $callback): self {}
    public function whereHas(string $relation): self {}
}
trait HasConvenientWhereMethods {
    public function whereIn(string $field, array $values): self {}
    public function whereNull(string $field): self {}
    public function whereAfter(string $field, string $date): self {}
}
/** @template TModel */
class SelectQueryBuilder {
    /** @use HasWhereQueryBuilderMethods<TModel> */
    use HasWhereQueryBuilderMethods;
    public function orderBy(string $field, string $direction = 'ASC'): self {}
    public function groupBy(string $statement): self {}
    public function having(string $statement, mixed ...$bindings): self {}
    public function include(string ...$fields): self {}
    public function sum(string $column): int {}
    /** @return TModel[] */
    public function all(): array {}
    /** @return TModel|null */
    public function first() {}
}
/** @template TModel */
class CountQueryBuilder {
    public function where(string $statement, mixed ...$bindings): self {}
}
/** @template TModel */
class UpdateQueryBuilder {
    public function where(string $statement, mixed ...$bindings): self {}
}
/** @template TModel */
class DeleteQueryBuilder {
    public function where(string $statement, mixed ...$bindings): self {}
}
/** @template TModel */
class WhereGroupBuilder {
    public function where(string $statement, mixed ...$bindings): self {}
}
namespace Tempest\Database;
trait IsDatabaseModel {
    public int $id;
    public static function new(mixed ...$params): static {}
    public static function create(mixed ...$params): static {}
    /** @return \Tempest\Database\Builder\QueryBuilders\SelectQueryBuilder<static> */
    public static function select(): \Tempest\Database\Builder\QueryBuilders\SelectQueryBuilder {}
    /** @return \Tempest\Database\Builder\QueryBuilders\SelectQueryBuilder<static> */
    public static function find(mixed ...$conditions): \Tempest\Database\Builder\QueryBuilders\SelectQueryBuilder {}
    public static function findOrNew(array $find, array $update): static {}
    public static function updateOrCreate(array $find, array $update): static {}
    public function update(mixed ...$params): static {}
}
/** @template TModel
 * @param TModel $model
 * @return \Tempest\Database\Builder\QueryBuilders\QueryBuilder<TModel>
 */
function query(string|object $model): \Tempest\Database\Builder\QueryBuilders\QueryBuilder {}
";

const MODELS: &str = r"<?php
namespace App;
trait BookFields { public string $isbn; }
class BaseBook {
    use \Tempest\Database\IsDatabaseModel;
    public string $title;
}
/** @property string $imaginary */
class Book extends BaseBook {
    use BookFields;
    public ?string $publishedAt;
    public string $INstock;
    public static string $staticField;
    protected string $protectedField;
    private string $privateField;
}

class Other { public string $different; }
class OverrideBook extends Book {
    public static function create(mixed ...$payload): static {}
}
class OverrideChild extends OverrideBook {}
class PrivateHelper extends Book {
    private static function find(mixed ...$conditions) {}
}
class Unrelated {
    public string $unrelated;
    public function where(string $statement, mixed ...$bindings) {}
    public function create(mixed ...$params) {}
}
namespace Caller;
class Book { public string $localTitle; }
";

async fn project(source: &str) -> (Backend, tempfile::TempDir, Uri, Uri) {
    let (backend, dir) = create_psr4_workspace(
        r#"{"require":{"tempest/database":"^3.0"},"autoload":{"psr-4":{"App\\":"app/","Tempest\\Database\\":"framework/"}}}"#,
        &[
            ("framework/Database.php", FRAMEWORK),
            ("app/Book.php", MODELS),
            (
                "global.php",
                "<?php class Book { public string $globalTitle; }",
            ),
            ("caller.php", source),
        ],
    );
    let models = workspace_uri(&backend, "app/Book.php");
    let uri = workspace_uri(&backend, "caller.php");
    open_php(
        &backend,
        &workspace_uri(&backend, "framework/Database.php"),
        FRAMEWORK,
    )
    .await;
    open_php(&backend, &models, MODELS).await;
    open_php(
        &backend,
        &workspace_uri(&backend, "global.php"),
        "<?php class Book { public string $globalTitle; }",
    )
    .await;
    open_php(&backend, &uri, source).await;
    (backend, dir, models, uri)
}

#[tokio::test]
async fn tempest_query_class_names_return_models_through_fluent_calls() {
    let source = r"<?php
namespace Caller;
use App\Book as ImportedBook;
use function Tempest\Database\query as build;
/** @param class-string<ImportedBook> $model */
function read(string $model): void {
    $literal = build(ImportedBook::class)->select()->where('title', 'PHP')->all();
    $instance = build(new ImportedBook())->select()->first();
    $variable = build($model)->select()->whereNull('publishedAt')->first();
    $named = build(model: ImportedBook::class)->select()->first();
    $global = \Tempest\Database\query(\Book::class)->select()->first();
    $literal[0]->;
    $instance->;
    $variable->;
    $named->;
    $global->;
}
";
    let (backend, _dir, _, uri) = project(source).await;
    for variable in ["$literal", "$instance", "$variable", "$named"] {
        let hover = hover_on(&backend, &uri, source, variable).await;
        assert!(hover.contains("Book"), "{variable}: {hover}");
        assert!(!hover.contains("class-string"), "{variable}: {hover}");
    }
    let hover = hover_on(&backend, &uri, source, "$global").await;
    assert!(hover.contains("Book"), "{hover}");
    assert!(!hover.contains("App\\Book"), "{hover}");
    assert!(!hover.contains("Caller\\Book"), "{hover}");
    assert!(!hover.contains("class-string"), "{hover}");
    for expression in [
        "$literal[0]->",
        "$instance->",
        "$variable->",
        "$named->",
        "$global->",
    ] {
        let at = position_after(source, expression);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        let global = expression == "$global->";
        assert_eq!(
            labels.contains(&"title".to_string()),
            !global,
            "{expression}: {labels:?}"
        );
        assert_eq!(
            labels.contains(&"globalTitle".to_string()),
            global,
            "{expression}: {labels:?}"
        );
        assert!(
            !labels.contains(&"localTitle".to_string()),
            "{expression}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn tempest_model_properties_cover_builders_and_explicit_generic_model_types() {
    let source = r"<?php
namespace Caller;
use App\Book as ImportedBook;
use Tempest\Database\Builder\QueryBuilders\QueryBuilder;
use Tempest\Database\Builder\QueryBuilders\WhereGroupBuilder;
use function Tempest\Database\query;
query(ImportedBook::class)->count('title')->where('', 'value');
query(ImportedBook::class)->count('');
query(ImportedBook::class)->select()->sum('');
query(ImportedBook::class)->update(title: 'given')->where('', 'value');
query(ImportedBook::class)->delete()->where('', 'value');
query(new ImportedBook())->select('id', '');
query(ImportedBook::class)->select()->include('');
query(ImportedBook::class)->insert();
query(ImportedBook::class)->update();
$anonymous = new class {
    use \Tempest\Database\IsDatabaseModel;
    public string $note;
};
$anonymous->create();
/** @param QueryBuilder<Book> $local
 * @param QueryBuilder<\Book> $global
 * @param QueryBuilder<ImportedBook> $imported
 * @param QueryBuilder<?ImportedBook> $nullable
 * @param QueryBuilder<ImportedBook|\App\Other> $union
 * @param QueryBuilder<'App\\Book'> $literal
 * @param WhereGroupBuilder<ImportedBook> $group
 */
function examples($local, $global, $imported, $nullable, $union, $literal, $group): void {
    $local->select('');
    $global->select('');
    $imported->select('');
    $nullable->select('');
    $union->select('');
    $literal->select('');
    $group->where('', 'value');
}
";
    let (backend, _dir, _, uri) = project(source).await;
    for needle in [
        "->count('",
        "->sum('",
        "count('title')->where('",
        "update(title: 'given')->where('",
        "delete()->where('",
        "select('id', '",
        "include('",
        "$imported->select('",
        "$nullable->select('",
        "$literal->select('",
        "$group->where('",
    ] {
        let at = position_after(source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        assert!(
            labels.contains(&"title".to_string()),
            "{needle}: {labels:?}"
        );
        assert!(
            !labels.contains(&"localTitle".to_string()),
            "{needle}: {labels:?}"
        );
    }
    let at = position_after(source, "$local->select('");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(labels, ["localTitle"]);
    let at = position_after(source, "$global->select('");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(labels, ["globalTitle"]);
    let at = position_after(source, "$anonymous->create(");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert!(labels.contains(&"note:".to_string()), "{labels:?}");
    let at = position_after(source, "$union->select('");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert!(
        labels.contains(&"title".to_string()) && labels.contains(&"different".to_string()),
        "{labels:?}"
    );
    for needle in ["->insert(", "query(ImportedBook::class)->update("] {
        let at = position_after(source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        assert!(
            labels.contains(&"isbn:".to_string()),
            "{needle}: {labels:?}"
        );
    }
}

#[tokio::test]
async fn tempest_model_properties_handle_edits_comments_and_used_keys() {
    let source = "<?php\r\nuse App\\Book;\r\nuse function Tempest\\Database\\query;\r\n\
        query(Book::class)\r\n/* chain */ ->select()->whereField(field: \"title\", value: 'text');\r\n\
        Book::findOrNew(['isbn' => 'php', 'title' => 'given'], ['publishedAt' => null]);\r\n\
        $book = new Book();\r\n$book->update(isbn: 'new');\r\n\
        Book::NEW(ti);\r\n";
    let (backend, _dir, model_uri, uri) = project(source).await;
    let at = position_after(source, "field: \"ti");
    let items = complete_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(items.len(), 1, "{items:?}");
    let Some(CompletionItemTextEdit::TextEdit(edit)) = &items[0].text_edit else {
        panic!("missing text edit")
    };
    assert_eq!(edit.new_text, "title");
    assert_eq!(edit.range.start, position_after(source, "field: \""));
    assert_eq!(edit.range.end, position_after(source, "field: \"title"));
    assert_eq!(
        definitions_at(&backend, &uri, source, "\"title\"").await[0].uri,
        model_uri
    );
    let at = position_after(source, "'php', '");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert!(labels.contains(&"title".to_string()));
    assert!(!labels.contains(&"isbn".to_string()));
    let at = position_after(source, "$book->update(");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert!(labels.contains(&"title:".to_string()));
    assert_eq!(
        definitions_at(&backend, &uri, source, "isbn:").await[0].uri,
        model_uri
    );
    let at = position_after(source, "Book::NEW(ti");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(labels, ["title:"]);
    let edited = source
        .replace("$title", "$heading")
        .replace("title", "heading");
    let models = MODELS.replace("$title", "$heading");
    open_php(&backend, &model_uri, &models).await;
    open_php(&backend, &uri, &edited).await;
    let at = position_after(&edited, "field: \"he");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(labels, ["heading"]);
    assert_eq!(
        definitions_at(&backend, &uri, &edited, "\"heading\"").await[0].uri,
        model_uri
    );
}

#[tokio::test]
async fn tempest_model_properties_require_a_matching_public_framework_method() {
    let source = r"<?php
use App\Book;
use function Tempest\Database\query;
query(Book::class)->select()->whereHas('');
query(Book::class)->select()->whereGroup('');
Book::select('');
Book::update();
\App\OverrideChild::create();
\App\PrivateHelper::find();
$unrelated = new \App\Unrelated();
$unrelated->where('', 1);
$unrelated->create();
$unknown->where('');
$scalar = 1;
$scalar->where('');
function create(mixed ...$params): void {}
create();
query(Book::class)->select()->whereMissing('');
query(Book::class)->select()->where('missing', 1);
Book::create(missing: 1);
Book::create('value');
Book::findOrNew(['title'], []);
";
    let (backend, _dir, _, uri) = project(source).await;
    for needle in [
        "whereHas('",
        "whereGroup('",
        "Book::select('",
        "Book::update(",
        "OverrideChild::create(",
        "PrivateHelper::find(",
        "$unrelated->where('",
        "$unrelated->create(",
        "$unknown->where('",
        "$scalar->where('",
        "\ncreate(",
        "whereMissing('",
    ] {
        let at = position_after(source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        assert!(
            !labels.iter().any(|label| matches!(
                label.as_str(),
                "title" | "title:" | "unrelated" | "unrelated:"
            )),
            "{needle}: {labels:?}"
        );
    }
    for needle in ["'missing'", "missing:"] {
        assert!(
            definitions_at(&backend, &uri, source, needle)
                .await
                .is_empty(),
            "{needle}"
        );
    }
    let at = position_after(source, "findOrNew(['ti");
    assert!(
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await)
            .is_empty()
    );
}

#[tokio::test]
async fn tempest_model_properties_reject_stale_call_offsets_and_complete_incomplete_calls() {
    let source = r"<?php
use App\Book;
use function Tempest\Database\query;
query(Book::class)->select()->where('title', 1);
";
    let (backend, _dir, _, uri) = project(source).await;
    backend.update_ast(uri.as_str(), &source.replace("'title'", "'isbn' "));
    assert!(
        definitions_at(&backend, &uri, source, "'title'")
            .await
            .is_empty()
    );
    let at = position_after(source, "where('ti");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(labels, ["title"]);
    let incomplete = r"<?php
use App\Book;
use function Tempest\Database\query;
query(Book::class)->select()->where('ti";
    open_php(&backend, &uri, incomplete).await;
    let at = position_after(incomplete, "where('ti");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(labels, ["title"]);
}

#[tokio::test]
async fn tempest_model_properties_complete_from_generic_builders_and_model_helpers() {
    let source = r"<?php
use App\Book;
use function Tempest\Database\query as table;
table(Book::class)->select()->whereIn('', []);
table(Book::class)->select()->whereIn('isbn', [])->where('')->whereNull('')->orderBy('');
table(Book::class)->select()->where('tit', 'value');
$builder = table(Book::class)->select();
$builder->whereAfter(field: '', date: '2026-01-01');
Book::select()->orderBy('');
Book::create();
Book::new(title: 'given', );
table(Book::class)->find();
Book::updateOrCreate(['' => 'php'], ['' => 'title']);
Book::findOrNew(update: array(''), find: ['isbn' => 'php']);
";
    let (backend, _dir, _, uri) = project(source).await;
    for (needle, named) in [
        ("whereIn('", false),
        ("whereIn('isbn', [])->where('", false),
        ("where('')->whereNull('", false),
        ("whereAfter(field: '", false),
        ("orderBy('", false),
        ("Book::create(", true),
        ("table(Book::class)->find(", true),
        ("Book::updateOrCreate(['", false),
        ("update: array('", false),
    ] {
        let at = position_after(source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        for name in ["id", "isbn", "title", "publishedAt"] {
            let expected = if named {
                format!("{name}:")
            } else {
                name.to_string()
            };
            assert!(
                labels.contains(&expected),
                "{needle}: missing {expected} in {labels:?}"
            );
        }
        for name in [
            "imaginary",
            "staticField",
            "protectedField",
            "privateField",
            "different",
        ] {
            assert!(
                !labels
                    .iter()
                    .any(|label| label.trim_end_matches(':') == name),
                "{needle}: {labels:?}"
            );
        }
    }
    let at = position_after(source, "where('tit");
    let items = complete_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].label, "title");
    let at = position_after(source, "Book::new(title: 'given', ");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert!(labels.contains(&"isbn:".to_string()));
    assert!(!labels.contains(&"title:".to_string()));
}

#[tokio::test]
async fn tempest_model_properties_navigate_to_inherited_and_trait_declarations() {
    let source = r"<?php
use App\Book;
use function Tempest\Database\query;
query(Book::class)->select()->whereIn('isbn', ['one']);
Book::select()->where('title', 'given');
Book::create(publishedAt: '2026-01-01');
Book::updateOrCreate(['isbn' => 'one'], ['title' => 'given']);
Book::findOrNew(update: ['publishedAt' => 'date'], find: ['isbn' => 'one']);
";
    let (backend, _dir, model_uri, uri) = project(source).await;
    for (needle, property) in [
        ("'isbn',", "$isbn"),
        ("'title',", "$title"),
        ("publishedAt:", "$publishedAt"),
        ("'isbn' =>", "$isbn"),
        ("'title' =>", "$title"),
        ("'publishedAt' =>", "$publishedAt"),
    ] {
        let locations = definitions_at(&backend, &uri, source, needle).await;
        assert_eq!(locations.len(), 1, "{needle}: {locations:?}");
        assert_eq!(locations[0].uri, model_uri);
        assert_eq!(
            locations[0].range.start.line,
            position_of(MODELS, property).line
        );
    }
}

#[tokio::test]
async fn tempest_model_properties_leave_sql_values_unrelated_calls_and_overrides_alone() {
    let source = r"<?php
use App\Book;
use function Tempest\Database\query;
query(Book::class)->select()->where('title = ?', 'value');
query(Book::class)->select()->where('title', 'title');
query(Book::class)->select()->whereRaw('title');
query(Book::class)->select()->groupBy('title');
query(Book::class)->select()->having('title');
query(Book::class)->select()->orderBy('title DESC');
query('books')->select()->where('title', 'value');
Book::updateOrCreate(['isbn' => 'title'], ['isbn' => ['title' => 1]]);
\App\OverrideBook::create();
";
    let (backend, _dir, _, uri) = project(source).await;
    for needle in [
        "where('title = ",
        "where('title', 'ti",
        "whereRaw('ti",
        "groupBy('ti",
        "having('ti",
        "orderBy('title ",
        "query('books')->select()->where('ti",
        "['isbn' => 'ti",
        "['isbn' => ['ti",
        "OverrideBook::create(",
    ] {
        let at = position_after(source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        assert!(
            !labels
                .iter()
                .any(|name| name == "title" || name == "title:"),
            "{needle}: {labels:?}"
        );
    }
    for needle in ["'title = ?'", "'title DESC'", "'title' =>"] {
        assert!(
            definitions_at(&backend, &uri, source, needle)
                .await
                .is_empty(),
            "{needle}"
        );
    }
    for needle in ["groupBy('ti", "having('ti"] {
        let at = position_after(source, needle);
        assert!(
            definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await)
                .is_empty()
        );
    }
}

#[tokio::test]
async fn tempest_model_properties_leave_word_operators_in_where_as_sql() {
    let source = r"<?php
use App\Book;
use function Tempest\Database\query;
query(Book::class)->select()->where('INstock', 1);
query(Book::class)->select()->whereField('INstock', 1);
Book::select()->where('', 1);
";
    let (backend, _dir, models, uri) = project(source).await;
    for needle in ["where('IN", "Book::select()->where('"] {
        let at = position_after(source, needle);
        let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
        assert!(!labels.contains(&"INstock".to_string()), "{labels:?}");
    }
    let at = position_after(source, "whereField('IN");
    let labels = complete_labels_at_opened(&backend, &uri, at.line, at.character).await;
    assert_eq!(labels, ["INstock"]);
    assert!(
        definitions_at(&backend, &uri, source, "'INstock'")
            .await
            .is_empty()
    );
    let locations =
        definition_locations(goto_definition_at(&backend, &uri, at.line, at.character).await);
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, models);
}
