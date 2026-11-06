//! A Doctrine query typed by its DQL: `createQuery()` with the query's
//! text, or `getQuery()` on a query builder, gives a `Query<TKey, TResult>`,
//! and the result methods hydrate `TResult`.  A builder carries the DQL its
//! calls build through a variable, a branch, and a method that returns it.

use crate::common::{
    complete_labels_at_opened, create_psr4_workspace, hover_text_at, open_document, split_cursor,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "doctrine/orm": "^3.0" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Doctrine\\ORM\\": "stubs/orm/"
    } }
}"#;

/// Just enough of Doctrine for the declared query types.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/orm/AbstractQuery.php",
        "<?php
namespace Doctrine\\ORM;
abstract class AbstractQuery {
    public const HYDRATE_OBJECT = 1;
    public const HYDRATE_ARRAY = 2;
    public function setParameter(string|int $key, mixed $value): static {}
    public function getResult(string|int $hydrationMode = self::HYDRATE_OBJECT): mixed {}
    public function getOneOrNullResult(string|int|null $hydrationMode = null): mixed {}
    public function getSingleResult(string|int|null $hydrationMode = null): mixed {}
    public function toIterable(iterable $parameters = [], $hydrationMode = self::HYDRATE_OBJECT): iterable {}
    public function execute($parameters = null, string|int|null $hydrationMode = null): mixed {}
}
",
    ),
    (
        "stubs/orm/Query.php",
        "<?php\nnamespace Doctrine\\ORM;\nfinal class Query extends AbstractQuery {\n    public function setMaxResults(?int $maxResults): self {}\n}\n",
    ),
    (
        "stubs/orm/QueryBuilder.php",
        "<?php
namespace Doctrine\\ORM;
class QueryBuilder {
    public function select(mixed ...$select): static {}
    public function addSelect(mixed ...$select): static {}
    public function from(string $from, string $alias, ?string $indexBy = null): static {}
    public function leftJoin(string $join, string $alias, ?string $conditionType = null, $condition = null, ?string $indexBy = null): static {}
    public function where(mixed ...$predicates): static {}
    public function andWhere(mixed ...$where): static {}
    public function setParameter(string|int $key, mixed $value): static {}
    public function delete(?string $delete = null, ?string $alias = null): static {}
    public function add(string $dqlPartName, string|object|array $dqlPart, bool $append = false): static {}
    public function resetDQLPart(string $part): static {}
    public function getQuery(): Query {}
}
",
    ),
    (
        "stubs/orm/Query/Expr/From.php",
        "<?php\nnamespace Doctrine\\ORM\\Query\\Expr;\nclass From {\n    public function __construct(protected string $from, protected string $alias, protected ?string $indexBy = null) {}\n}\n",
    ),
    (
        "stubs/orm/EntityManagerInterface.php",
        "<?php\nnamespace Doctrine\\ORM;\ninterface EntityManagerInterface {\n    public function createQuery(string $dql = ''): Query;\n    public function createQueryBuilder(): QueryBuilder;\n}\n",
    ),
    (
        "stubs/orm/EntityRepository.php",
        "<?php\nnamespace Doctrine\\ORM;\n/** @template T of object */\nclass EntityRepository {\n    public function createQueryBuilder(string $alias, ?string $indexBy = null): QueryBuilder {}\n}\n",
    ),
    (
        "stubs/orm/Mapping/Entity.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nfinal class Entity {\n    public function __construct(public readonly ?string $repositoryClass = null) {}\n}\n",
    ),
    (
        "stubs/orm/Mapping/Column.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute(\\Attribute::TARGET_PROPERTY)]\nfinal class Column {\n    public function __construct(public readonly ?string $type = null, public readonly bool $nullable = false) {}\n}\n",
    ),
    (
        "stubs/orm/Mapping/ManyToOne.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute(\\Attribute::TARGET_PROPERTY)]\nfinal class ManyToOne {\n    public function __construct(public readonly ?string $targetEntity = null) {}\n}\n",
    ),
    (
        "src/Entity/User.php",
        "<?php
namespace App\\Entity;
use App\\Repository\\UserRepository;
use Doctrine\\ORM\\Mapping as ORM;
#[ORM\\Entity(repositoryClass: UserRepository::class)]
class User {
    #[ORM\\Column]
    public int $id;
    #[ORM\\Column]
    public string $email;
    #[ORM\\Column(nullable: true)]
    public ?string $name;
    #[ORM\\ManyToOne]
    public ?Address $address;
    public function greet(): string {}
}
",
    ),
    (
        "src/Entity/Address.php",
        "<?php
namespace App\\Entity;
use Doctrine\\ORM\\Mapping as ORM;
#[ORM\\Entity]
class Address {
    #[ORM\\Column]
    public string $city;
}
",
    ),
    (
        "src/Dto/UserSummary.php",
        "<?php\nnamespace App\\Dto;\nfinal class UserSummary {\n    public function __construct(public int $id, public string $email) {}\n}\n",
    ),
    (
        "src/Repository/UserRepository.php",
        "<?php
namespace App\\Repository;
use App\\Entity\\User;
/** @extends \\Doctrine\\ORM\\EntityRepository<User> */
class UserRepository extends \\Doctrine\\ORM\\EntityRepository {
    public function named(): \\Doctrine\\ORM\\QueryBuilder {
        $qb = $this->createQueryBuilder('u')->select('u.name');
        $qb->andWhere('u.name IS NOT NULL');
        return $qb;
    }
    public function active(): void {
        $users = $this->createQueryBuilder('u')->andWhere('u.name IS NOT NULL')->getQuery()->getResult();
        $us§ers;
    }
}
",
    ),
];

/// A workspace with the Doctrine stubs and the entities, and a service
/// whose method body is `body`.  The `§` marker is in `body`, or, when
/// `body` is `None`, in the repository stub.
async fn open_service(body: Option<&str>) -> (Backend, tempfile::TempDir, Uri, Position) {
    let source = format!(
        "<?php
namespace App\\Service;

use App\\Entity\\User;
use App\\Repository\\UserRepository;
use Doctrine\\ORM\\AbstractQuery;
use Doctrine\\ORM\\EntityManagerInterface;

final class Users
{{
    public function run(EntityManagerInterface $em, UserRepository $users, string $dql): void
    {{
{}
    }}
}}
",
        body.unwrap_or("")
    );
    let mut files: Vec<(&str, String)> = STUBS
        .iter()
        .map(|(path, text)| (*path, text.to_string()))
        .collect();
    let service = "src/Service/Users.php";
    let repository = "src/Repository/UserRepository.php";
    let (open_path, position) = match body {
        Some(_) => {
            let (stripped, position) = split_cursor(&source);
            files.push((service, stripped));
            (service, position)
        }
        None => {
            files.push((service, source.clone()));
            let entry = files.iter_mut().find(|(p, _)| *p == repository).unwrap();
            let (stripped, position) = split_cursor(&entry.1);
            entry.1 = stripped;
            (repository, position)
        }
    };
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &refs);
    let text = files
        .iter()
        .find(|(p, _)| *p == open_path)
        .unwrap()
        .1
        .clone();
    let uri = Uri::from_file_path(dir.path().join(open_path)).unwrap();
    open_document(&backend, &uri, "php", &text).await;
    (backend, dir, uri, position)
}

/// The hover at the `§` marker in a service method whose body is `body`.
async fn hover(body: &str) -> String {
    let (backend, _dir, uri, position) = open_service(Some(body)).await;
    hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default()
}

/// Whether a variable's hover shows it typed as exactly `ty`.
fn shows(hover: &str, ty: &str) -> bool {
    hover.contains(&format!(" = {ty}\n"))
}

#[tokio::test]
async fn a_query_is_typed_by_the_entity_it_selects() {
    let hover = hover(
        "        $query = $em->createQuery('SELECT u FROM App\\Entity\\User u');\n        $qu§ery;",
    )
    .await;
    assert!(shows(&hover, "Query<null, User>"), "{hover}");
}

#[tokio::test]
async fn get_result_is_a_list_of_the_entity() {
    let hover = hover(
        "        $users = $em->createQuery('SELECT u FROM ' . User::class . ' u')->getResult();\n        $us§ers;",
    )
    .await;
    assert!(shows(&hover, "list<User>"), "{hover}");
}

#[tokio::test]
async fn selected_fields_give_a_shape_typed_by_their_properties() {
    let hover = hover(
        "        $rows = $em->createQuery('SELECT u.id, u.name AS label, a.city FROM App\\Entity\\User u LEFT JOIN u.address a')->getResult();\n        $ro§ws;",
    )
    .await;
    assert!(
        shows(
            &hover,
            "list<array{id: int, label: ?string, city: string|null}>"
        ),
        "{hover}"
    );
}

#[tokio::test]
async fn aggregates_and_functions_are_typed_where_every_driver_agrees() {
    let rows = hover(
        "        $rows = $em->createQuery(\"SELECT u.email, COUNT(a.city) AS total, LOWER(u.name), CASE WHEN u.id > 1 THEN 'old' ELSE 'new' END age, SUM(u.id) AS ids FROM App\\Entity\\User u LEFT JOIN u.address a GROUP BY u.email\")->getResult();\n        $ro§ws;",
    )
    .await;
    assert!(
        shows(
            &rows,
            "list<array{email: string, total: int<0, max>, 1: string|null, age: 'old'|'new', ids: mixed}>"
        ),
        "{rows}"
    );

    // Without a `GROUP BY`, the one row may come from no row at all.
    let row = hover(
        "        $row = $em->createQuery('SELECT COUNT(u.id) AS total, u.email FROM App\\Entity\\User u')->getSingleResult(AbstractQuery::HYDRATE_OBJECT);\n        $r§ow;",
    )
    .await;
    assert!(
        shows(&row, "array{total: int<0, max>, email: string|null}"),
        "{row}"
    );
}

#[tokio::test]
async fn a_new_object_is_the_row() {
    let hover = hover(
        "        $rows = $em->createQuery('SELECT NEW App\\Dto\\UserSummary(u.id, u.email) FROM App\\Entity\\User u')->getResult();\n        $ro§ws;",
    )
    .await;
    assert!(shows(&hover, "list<UserSummary>"), "{hover}");
}

#[tokio::test]
async fn index_by_keys_the_result_by_the_field() {
    let rows = hover(
        "        $rows = $em->createQuery('SELECT u.name FROM App\\Entity\\User u INDEX BY u.email')->getResult();\n        $ro§ws;",
    )
    .await;
    // Doctrine keys rows of fields only by a field they select.
    assert!(shows(&rows, "list<array{name: ?string}>"), "{rows}");

    let hover = hover(
        "        $users = $em->createQuery('SELECT u FROM App\\Entity\\User u INDEX BY u.email')->getResult();\n        $us§ers;",
    )
    .await;
    assert!(shows(&hover, "array<string, User>"), "{hover}");
}

#[tokio::test]
async fn the_hydration_mode_decides_what_a_result_method_returns() {
    let object = hover(
        "        $query = $em->createQuery('SELECT u FROM App\\Entity\\User u')->setMaxResults(1);\n        $user = $query->getOneOrNullResult(AbstractQuery::HYDRATE_OBJECT);\n        $us§er;",
    )
    .await;
    assert!(shows(&object, "User|null"), "{object}");

    let array = hover(
        "        $query = $em->createQuery('SELECT u FROM App\\Entity\\User u');\n        $rows = $query->getResult(AbstractQuery::HYDRATE_ARRAY);\n        $ro§ws;",
    )
    .await;
    assert!(shows(&array, "mixed"), "{array}");
}

#[tokio::test]
async fn an_update_or_delete_counts_the_affected_rows() {
    let hover = hover(
        "        $count = $em->createQuery('DELETE FROM App\\Entity\\User u WHERE u.id = 1')->execute();\n        $cou§nt;",
    )
    .await;
    assert!(shows(&hover, "int<0, max>"), "{hover}");
}

#[tokio::test]
async fn dql_that_is_not_known_gives_a_mixed_query() {
    let hover = hover("        $query = $em->createQuery($dql);\n        $qu§ery;").await;
    assert!(shows(&hover, "Query<mixed, mixed>"), "{hover}");
}

#[tokio::test]
async fn a_query_builder_chain_is_typed_by_the_dql_it_builds() {
    let hover = hover(
        "        $rows = $em->createQueryBuilder()\n            ->select('u.id', 'u.email')\n            ->addSelect('a.city')\n            ->from(User::class, 'u', 'u.id')\n            ->leftJoin('u.address', 'a')\n            ->where($dql)\n            ->getQuery()\n            ->getResult();\n        $ro§ws;",
    )
    .await;
    assert!(
        shows(
            &hover,
            "array<int, array{id: int, email: string, city: string|null}>"
        ),
        "{hover}"
    );
}

#[tokio::test]
async fn a_builder_call_with_unknown_arguments_gives_a_mixed_query() {
    let hover = hover(
        "        $query = $em->createQueryBuilder()->select('u')->from($dql, 'u')->getQuery();\n        $qu§ery;",
    )
    .await;
    assert!(shows(&hover, "Query<mixed, mixed>"), "{hover}");
}

#[tokio::test]
async fn a_repository_query_builder_selects_its_entity() {
    let hover = hover(
        "        $users = $users->createQueryBuilder('u')->setParameter('a', 1)->getQuery()->getResult();\n        $us§ers;",
    )
    .await;
    assert!(shows(&hover, "list<User>"), "{hover}");
}

#[tokio::test]
async fn a_repository_builds_queries_for_its_own_entity() {
    let (backend, _dir, uri, position) = open_service(None).await;
    let hover = hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(shows(&hover, "list<User>"), "{hover}");
}

#[tokio::test]
async fn a_builder_delete_counts_the_affected_rows() {
    let hover = hover(
        "        $count = $users->createQueryBuilder('u')->delete()->getQuery()->execute();\n        $cou§nt;",
    )
    .await;
    assert!(shows(&hover, "int<0, max>"), "{hover}");
}

#[tokio::test]
async fn a_result_row_completes_the_entity_members() {
    let (backend, _dir, uri, position) = open_service(Some(
        "        foreach ($em->createQuery('SELECT u FROM App\\Entity\\User u')->getResult() as $user) {\n            $user->§\n        }",
    ))
    .await;
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert!(labels.iter().any(|l| l.starts_with("greet")), "{labels:?}");
}

#[tokio::test]
async fn a_chained_result_completes_the_entity_members() {
    let (backend, _dir, uri, position) = open_service(Some(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User u')->getSingleResult(AbstractQuery::HYDRATE_OBJECT)->§",
    ))
    .await;
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert!(labels.iter().any(|l| l.starts_with("greet")), "{labels:?}");
}

#[tokio::test]
async fn a_chained_builder_result_completes_the_entity_members() {
    let (backend, _dir, uri, position) = open_service(Some(
        "        $em->createQueryBuilder()->select('u')->from(User::class, 'u')->getQuery()->getOneOrNullResult(AbstractQuery::HYDRATE_OBJECT)->§",
    ))
    .await;
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert!(labels.iter().any(|l| l.starts_with("greet")), "{labels:?}");
}

#[tokio::test]
async fn a_builder_in_a_variable_takes_the_calls_made_on_it() {
    let hover = hover(
        "        $qb = $em->createQueryBuilder();\n        $qb->select('u')->from(User::class, 'u');\n        $qb->andWhere('u.id = 1');\n        $users = $qb->getQuery()->getResult();\n        $us§ers;",
    )
    .await;
    assert!(shows(&hover, "list<User>"), "{hover}");
}

#[tokio::test]
async fn a_builder_changed_in_a_branch_gives_a_query_for_each() {
    let hover = hover(
        "        $qb = $em->createQueryBuilder();\n        if ($dql) {\n            $qb->select('u.id');\n        } else {\n            $qb->select('u.id', 'u.email');\n        }\n        $query = $qb->from(User::class, 'u')->getQuery();\n        $qu§ery;",
    )
    .await;
    assert!(shows(&hover, "Query<null, array{id: int}>"), "{hover}");
    assert!(
        shows(&hover, "Query<null, array{id: int, email: string}>"),
        "{hover}"
    );
}

#[tokio::test]
async fn a_builder_a_method_returns_carries_its_dql() {
    let hover =
        hover("        $rows = $users->named()->getQuery()->getResult();\n        $ro§ws;").await;
    assert!(shows(&hover, "list<array{name: ?string}>"), "{hover}");
}

#[tokio::test]
async fn a_call_that_is_not_replayed_forgets_the_dql() {
    let hover = hover(
        "        $qb = $users->createQueryBuilder('u');\n        $qb->resetDQLPart('select');\n        $query = $qb->getQuery();\n        $qu§ery;",
    )
    .await;
    assert!(shows(&hover, "Query<mixed, mixed>"), "{hover}");
}

#[tokio::test]
async fn an_expression_object_added_to_a_builder_is_replayed() {
    let hover = hover(
        "        $rows = $em->createQueryBuilder()\n            ->select('u.email')\n            ->add('from', new \\Doctrine\\ORM\\Query\\Expr\\From(User::class, 'u'), true)\n            ->getQuery()\n            ->getResult();\n        $ro§ws;",
    )
    .await;
    assert!(shows(&hover, "list<array{email: string}>"), "{hover}");
}
