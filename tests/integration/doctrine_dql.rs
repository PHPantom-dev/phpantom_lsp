//! The entity classes and fields Doctrine Query Language names, in a query
//! and in the fragments a query builder joins into one.

use crate::common::{
    complete_labels_at_opened, create_psr4_workspace, find_references, goto_definition_at,
    hover_text_at, message_text, open_document, split_cursor,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "doctrine/orm": "^3.0" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Doctrine\\ORM\\": "stubs/orm/",
        "Doctrine\\DBAL\\": "stubs/dbal/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[
    (
        "stubs/orm/EntityRepository.php",
        "<?php
namespace Doctrine\\ORM;
/** @template T of object */
class EntityRepository {
    public function createQueryBuilder(string $alias, ?string $indexBy = null): QueryBuilder {}
    public function __call(string $method, array $arguments): mixed {}
}
",
    ),
    (
        "stubs/orm/EntityManagerInterface.php",
        "<?php
namespace Doctrine\\ORM;
interface EntityManagerInterface {
    public function createQuery(string $dql = ''): Query;
    public function createQueryBuilder(): QueryBuilder;
}
",
    ),
    (
        "stubs/orm/Query.php",
        "<?php
namespace Doctrine\\ORM;
class Query {
    public function getResult(): mixed {}
    public function setDQL(string $dql): static {}
}
",
    ),
    (
        "stubs/orm/QueryBuilder.php",
        "<?php
namespace Doctrine\\ORM;
class QueryBuilder {
    public function expr(): Query\\Expr {}
    public function select(mixed ...$select): static {}
    public function from(string $from, string $alias, ?string $indexBy = null): static {}
    public function join(string $join, string $alias, ?string $conditionType = null, mixed $condition = null, ?string $indexBy = null): static {}
    public function leftJoin(string $join, string $alias, ?string $conditionType = null, mixed $condition = null, ?string $indexBy = null): static {}
    public function where(mixed ...$predicates): static {}
    public function andWhere(mixed ...$where): static {}
    public function groupBy(string ...$groupBy): static {}
    public function having(mixed ...$having): static {}
    public function set(string $key, mixed $value): static {}
    public function orderBy(string $sort, ?string $order = null): static {}
    public function add(string $dqlPartName, string|object|array $dqlPart, bool $append = false): static {}
    public function getQuery(): Query {}
}
",
    ),
    (
        "stubs/dbal/Query/QueryBuilder.php",
        "<?php
namespace Doctrine\\DBAL\\Query;
class QueryBuilder {
    public function select(string ...$expressions): static {}
    public function from(string $table, ?string $alias = null): static {}
    public function where(mixed ...$predicates): static {}
}
",
    ),
    (
        "stubs/orm/Query/Expr.php",
        "<?php
namespace Doctrine\\ORM\\Query;
class Expr {
    public function isNull(string $x): string {}
    public function in(string $x, mixed $y): object {}
    public function exists(mixed $subquery): object {}
    public function orX(mixed ...$x): object {}
    public function eq(mixed $x, mixed $y): object {}
    public function literal(mixed $literal): object {}
}
",
    ),
    (
        "stubs/orm/Query/Expr/OrderBy.php",
        "<?php
namespace Doctrine\\ORM\\Query\\Expr;
class OrderBy {
    public function __construct(?string $sort = null, ?string $order = null) {}
}
",
    ),
    (
        "stubs/orm/Query/Expr/Andx.php",
        "<?php
namespace Doctrine\\ORM\\Query\\Expr;
class Andx {
    public function __construct(mixed $args = []) {}
}
",
    ),
    (
        "stubs/orm/Mapping/Entity.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute]\nfinal class Entity { public function __construct(public ?string $repositoryClass = null) {} }\n",
    ),
    (
        "stubs/orm/Mapping/Column.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute]\nfinal class Column { public function __construct(public ?string $type = null) {} }\n",
    ),
    (
        "stubs/orm/Mapping/Id.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute]\nfinal class Id {}\n",
    ),
    (
        "stubs/orm/Mapping/ManyToOne.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute]\nfinal class ManyToOne { public function __construct(public ?string $targetEntity = null) {} }\n",
    ),
    (
        "stubs/orm/Mapping/OneToMany.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute]\nfinal class OneToMany { public function __construct(public ?string $targetEntity = null, public ?string $mappedBy = null) {} }\n",
    ),
    (
        "src/Entity/User.php",
        "<?php
namespace App\\Entity;
use App\\Repository\\UserRepository;
use Doctrine\\ORM\\Mapping as ORM;
#[ORM\\Entity(repositoryClass: UserRepository::class)]
class User {
    #[ORM\\Id]
    #[ORM\\Column]
    private int $id;
    #[ORM\\Column]
    private string $email;
    #[ORM\\ManyToOne(targetEntity: Team::class)]
    private Team $team;
    private string $plainPassword;
}
",
    ),
    (
        "src/Entity/Team.php",
        "<?php
namespace App\\Entity;
use Doctrine\\ORM\\Mapping as ORM;
#[ORM\\Entity]
class Team {
    #[ORM\\Id]
    #[ORM\\Column]
    private int $id;
    #[ORM\\Column]
    private string $name;
    #[ORM\\ManyToOne]
    private ?User $owner;
    #[ORM\\OneToMany(targetEntity: 'User', mappedBy: 'team')]
    private iterable $members;
}
",
    ),
    // The entity is spelled in full: a body read from a file loaded on
    // demand does not see that file's imports yet.
    (
        "src/Repository/Queries.php",
        "<?php
namespace App\\Repository;
use Doctrine\\ORM\\EntityManagerInterface;
use Doctrine\\ORM\\QueryBuilder;
final class Queries {
    public function __construct(private EntityManagerInterface $em) {}
    public function users(): QueryBuilder
    {
        $qb = $this->em->createQueryBuilder();
        $qb->select('u')->from(\\App\\Entity\\User::class, 'u');
        return $qb;
    }
}
",
    ),
    (
        "src/Repository/UserRepository.php",
        "<?php
namespace App\\Repository;
use App\\Entity\\User;
/** @extends \\Doctrine\\ORM\\EntityRepository<User> */
class UserRepository extends \\Doctrine\\ORM\\EntityRepository {
    public function active(): array
    {
        return $this->createQueryBuilder('u')
            ->where('u.email IS NOT NULL')
            ->getQuery()
            ->getResult();
    }
}
",
    ),
];

/// A workspace with the Doctrine stubs and entities, and a service whose
/// method body is `body`, opened at the `§` marker.
async fn open_service(body: &str) -> (Backend, tempfile::TempDir, Uri, String, Position) {
    open_service_with(
        &format!(
            "    public function run(UserRepository $users, EntityManagerInterface $em): void
    {{
{body}
    }}"
        ),
        &[],
    )
    .await
}

/// A workspace with the Doctrine stubs and entities, the files `extra`,
/// and a service class whose members are `members`, opened at the `§`
/// marker.
async fn open_service_with(
    members: &str,
    extra: &[(&str, &str)],
) -> (Backend, tempfile::TempDir, Uri, String, Position) {
    let source = format!(
        "<?php
namespace App\\Service;

use App\\Entity\\Team;
use App\\Entity\\User;
use App\\Repository\\UserRepository;
use Doctrine\\ORM\\EntityManagerInterface;
use Doctrine\\ORM\\QueryBuilder;

final class Users extends BaseUsers
{{
{members}
}}
"
    );
    let (stripped, position) = split_cursor(&source);
    let path = "src/Service/Users.php";
    let mut files: Vec<(&str, &str)> = STUBS.to_vec();
    files.extend_from_slice(extra);
    if !extra.iter().any(|(p, _)| *p == "src/Service/BaseUsers.php") {
        files.push((
            "src/Service/BaseUsers.php",
            "<?php\nnamespace App\\Service;\nabstract class BaseUsers {}\n",
        ));
    }
    files.push((path, stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
    let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
    open_document(&backend, &uri, "php", &stripped).await;
    (backend, dir, uri, stripped, position)
}

async fn completions(body: &str) -> Vec<String> {
    let (backend, _dir, uri, _, position) = open_service(body).await;
    complete_labels_at_opened(&backend, &uri, position.line, position.character).await
}

/// The unknown-class, unknown-member and unknown-field messages for a
/// service whose method body is `body`.
async fn diagnostics(body: &str) -> Vec<String> {
    let (backend, _dir, uri, content, _) = open_service(&format!("{body}§")).await;
    let mut out = Vec::new();
    backend.collect_unknown_class_diagnostics(uri.as_str(), &content, &mut out);
    backend.collect_unknown_member_diagnostics(uri.as_str(), &content, &mut out);
    out.iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

fn unmapped(field: &str, entity: &str) -> String {
    format!("Field '{field}' is not mapped on entity App\\Entity\\{entity}")
}

#[tokio::test]
async fn a_query_reports_unmapped_fields_and_unknown_entities() {
    let messages = diagnostics(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE u.email = :e AND u.plainPassword = 1');
        $em->createQuery('SELECT t FROM ' . Team::class . ' t ORDER BY t.size');
        $em->createQuery('SELECT f FROM App\\Entity\\Missing f WHERE f.anything = 1');
        $em->createQuery(\"SELECT u, t FROM App\\\\Entity\\\\User u JOIN App\\Entity\\Team t WITH t.id = u.team WHERE t.nope = 1\");",
    )
    .await;
    assert_eq!(
        messages,
        vec![
            "Class App\\Entity\\Missing not found".to_string(),
            unmapped("plainPassword", "User"),
            unmapped("size", "Team"),
            unmapped("nope", "Team"),
        ]
    );
}

#[tokio::test]
async fn a_query_builder_reports_unmapped_fields() {
    let messages = diagnostics(
        "        $qb = $em->createQueryBuilder();
        $qb->select('u')->from(User::class, 'u');
        $qb->andWhere('u.email = :e')->andWhere($qb->expr()->isNull('u.nickname'));
        $qb->orderBy('u.name', 'ASC');
        $users->createQueryBuilder('x')->where('x.plainPassword = 1')->andWhere('x.id = 1');",
    )
    .await;
    assert_eq!(
        messages,
        vec![
            unmapped("nickname", "User"),
            unmapped("name", "User"),
            unmapped("plainPassword", "User"),
        ]
    );
}

#[tokio::test]
async fn variables_that_stand_for_nothing_known_are_left_alone() {
    let messages = diagnostics(
        "        $em->createQuery('SELECT u FROM ' . $entity . ' u WHERE u.anything = 1');
        $em->createQueryBuilder()->select('t')->from(User::class, 't')->leftJoin('t.email', 'e')->where('e.anything = 1');
        $other = $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE u.' . $field . ' = 1');",
    )
    .await;
    assert!(messages.is_empty(), "{messages:?}");
}

#[tokio::test]
async fn a_variable_joined_through_an_association_stands_for_its_target() {
    let messages = diagnostics(
        "        $em->createQuery('SELECT u, t FROM App\\Entity\\User u JOIN u.team t JOIN t.owner o WHERE t.name = 1 AND t.nope = 1 AND o.size = 1');
        $em->createQueryBuilder()->select('u')->from(User::class, 'u')->leftJoin('u.team', 'team')->where('team.anything = 1');
        $users->createQueryBuilder('x')->join('x.team', 't')->join('t.members', 'm')->where('m.email = 1 AND m.colour = 1');",
    )
    .await;
    assert_eq!(
        messages,
        vec![
            unmapped("nope", "Team"),
            unmapped("size", "User"),
            unmapped("anything", "Team"),
            unmapped("colour", "User"),
        ]
    );
}

#[tokio::test]
async fn a_joined_variable_s_field_completes_and_goes_to_the_property() {
    let labels = completions(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User u JOIN u.team t WHERE t.§');",
    )
    .await;
    assert!(labels.contains(&"name".to_string()), "{labels:?}");
    assert!(!labels.contains(&"email".to_string()), "{labels:?}");

    let (backend, dir, uri, _, position) = open_service(
        "        $users->createQueryBuilder('u')->join('u.team', 't')->where('t.na§me = 1');",
    )
    .await;
    let Some(DefinitionResponse::Definition(Definition::Location(location))) =
        goto_definition_at(&backend, &uri, position.line, position.character).await
    else {
        panic!("no definition");
    };
    let team = Uri::from_file_path(dir.path().join("src/Entity/Team.php")).unwrap();
    assert_eq!(location.uri, team);
    assert_eq!(location.range.start.line, 9);
}

#[tokio::test]
async fn a_variable_declared_differently_in_one_body_stands_for_nothing() {
    let messages = diagnostics(
        "        $em->createQueryBuilder()->from(User::class, 'e')->where('e.name = 1');
        $em->createQueryBuilder()->from(Team::class, 'e')->where('e.email = 1');",
    )
    .await;
    assert!(messages.is_empty(), "{messages:?}");
}

#[tokio::test]
async fn a_builder_another_method_returns_reads_the_variables_it_declared() {
    let messages = diagnostics(
        "        $queries = new \\App\\Repository\\Queries($em);
        $queries->users()->andWhere('u.nope = 1');
        $queries->users()->leftJoin('u.team', 't')->andWhere('t.size = 1 AND t.name = 1');
        $qb = $queries->users();
        $qb->andWhere('u.colour = 1')->andWhere($qb->expr()->isNull('u.shade'));
        $queries->users()->andWhere('x.anything = 1');",
    )
    .await;
    assert_eq!(
        messages,
        vec![
            unmapped("nope", "User"),
            unmapped("size", "Team"),
            unmapped("colour", "User"),
            unmapped("shade", "User"),
        ]
    );

    let (backend, dir, uri, _, position) = open_service(
        "        (new \\App\\Repository\\Queries($em))->users()->andWhere('u.em§ail = 1');",
    )
    .await;
    let Some(DefinitionResponse::Definition(Definition::Location(location))) =
        goto_definition_at(&backend, &uri, position.line, position.character).await
    else {
        panic!("no definition");
    };
    let user = Uri::from_file_path(dir.path().join("src/Entity/User.php")).unwrap();
    assert_eq!(location.uri, user);
    assert_eq!(location.range.start.line, 10);
}

#[tokio::test]
async fn an_expression_object_reports_unmapped_fields() {
    let messages = diagnostics(
        "        $order = new \\Doctrine\\ORM\\Query\\Expr\\OrderBy('u.size', 'ASC');
        $em->createQueryBuilder()->select('u')->from(User::class, 'u')
            ->add('where', new \\Doctrine\\ORM\\Query\\Expr\\Andx(['u.email = 1', 'u.nope = 1']))
            ->add('orderBy', $order);",
    )
    .await;
    assert_eq!(
        messages,
        vec![unmapped("size", "User"), unmapped("nope", "User")]
    );
}

#[tokio::test]
async fn a_repository_query_builder_reads_its_own_entity() {
    let (backend, dir, _, _, _) = open_service("§").await;
    let uri = Uri::from_file_path(dir.path().join("src/Repository/UserRepository.php")).unwrap();
    let content = std::fs::read_to_string(dir.path().join("src/Repository/UserRepository.php"))
        .unwrap()
        .replace("u.email", "u.mail");
    open_document(&backend, &uri, "php", &content).await;
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri.as_str(), &content, &mut out);
    let messages: Vec<&str> = out.iter().map(|d| message_text(&d.message)).collect();
    assert_eq!(messages, vec![unmapped("mail", "User").as_str()]);
}

#[tokio::test]
async fn a_field_goes_to_the_property() {
    let (backend, dir, uri, _, position) = open_service(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE u.em§ail = :e');",
    )
    .await;
    let Some(DefinitionResponse::Definition(Definition::Location(location))) =
        goto_definition_at(&backend, &uri, position.line, position.character).await
    else {
        panic!("no definition");
    };
    let user = Uri::from_file_path(dir.path().join("src/Entity/User.php")).unwrap();
    assert_eq!(location.uri, user);
    assert_eq!(location.range.start.line, 10);
}

#[tokio::test]
async fn an_entity_name_goes_to_the_class() {
    let (backend, dir, uri, _, position) =
        open_service("        $em->createQuery('SELECT u FROM App\\Entity\\Us§er u');").await;
    let Some(DefinitionResponse::Definition(Definition::Location(location))) =
        goto_definition_at(&backend, &uri, position.line, position.character).await
    else {
        panic!("no definition");
    };
    let user = Uri::from_file_path(dir.path().join("src/Entity/User.php")).unwrap();
    assert_eq!(location.uri, user);
}

#[tokio::test]
async fn a_query_builder_field_shows_the_property_on_hover() {
    let (backend, _dir, uri, _, position) = open_service(
        "        $em->createQueryBuilder()->select('u')->from(User::class, 'u')->where('u.em§ail = 1');",
    )
    .await;
    let hover = hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("$email"), "{hover}");
}

#[tokio::test]
async fn a_field_is_a_reference_to_the_property() {
    let (backend, dir, uri, _, position) = open_service(
        "        $users->createQueryBuilder('u')->where('u.em§ail = 1');
        $em->createQuery('SELECT x FROM App\\Entity\\User x WHERE x.email = 1');",
    )
    .await;
    let locations = find_references(&backend, &uri, position, true).await;
    let user = Uri::from_file_path(dir.path().join("src/Entity/User.php")).unwrap();
    assert!(
        locations.iter().any(|l| l.uri == user),
        "the property is missing from {locations:?}"
    );
    assert_eq!(
        locations.iter().filter(|l| l.uri == uri).count(),
        2,
        "{locations:?}"
    );
}

#[tokio::test]
async fn renaming_the_property_renames_the_fields() {
    let (backend, _dir, uri, content, position) = open_service(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE u.em§ail = :e ORDER BY u.email');",
    )
    .await;
    let edit = crate::common::rename(&backend, &uri, position.line, position.character, "mail")
        .await
        .expect("no rename edit");
    let fields = crate::common::edits_for_uri(&edit, &uri);
    assert_eq!(fields.len(), 2, "{fields:?}");
    for field in &fields {
        let line = content
            .lines()
            .nth(field.range.start.line as usize)
            .unwrap();
        let start = field.range.start.character as usize;
        let end = field.range.end.character as usize;
        assert_eq!(&line[start..end], "email", "{fields:?}");
    }
}

#[tokio::test]
async fn a_field_completes_after_its_variable() {
    let labels =
        completions("        $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE u.§');")
            .await;
    assert!(labels.contains(&"email".to_string()), "{labels:?}");
    assert!(labels.contains(&"team".to_string()), "{labels:?}");
    assert!(!labels.contains(&"plainPassword".to_string()), "{labels:?}");
}

#[tokio::test]
async fn a_query_builder_field_completes_after_its_variable() {
    let labels = completions(
        "        $qb = $users->createQueryBuilder('u');
        $qb->andWhere('u.em§');",
    )
    .await;
    assert_eq!(labels, vec!["email".to_string()]);
}

/// Completion at the `§` marker of a body, with the entity and repository
/// files opened so the workspace knows their classes.
async fn completions_with_classes_known(body: &str) -> Vec<String> {
    let (backend, dir, uri, _, position) = open_service(body).await;
    for path in [
        "src/Entity/User.php",
        "src/Entity/Team.php",
        "src/Repository/UserRepository.php",
    ] {
        let file = dir.path().join(path);
        let text = std::fs::read_to_string(&file).unwrap();
        open_document(&backend, &Uri::from_file_path(file).unwrap(), "php", &text).await;
    }
    complete_labels_at_opened(&backend, &uri, position.line, position.character).await
}

#[tokio::test]
async fn an_entity_name_completes_after_from() {
    let labels = completions_with_classes_known(
        "        $em->createQuery('SELECT u FROM App\\Entity\\U§');",
    )
    .await;
    assert!(labels.iter().any(|l| l.ends_with("User")), "{labels:?}");
    assert!(!labels.iter().any(|l| l.ends_with("Team")), "{labels:?}");
    let labels = completions_with_classes_known(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User u JOIN App\\Repository\\U§');",
    )
    .await;
    assert!(labels.is_empty(), "{labels:?}");
}

#[tokio::test]
async fn renaming_the_entity_class_renames_its_name_in_dql() {
    let (backend, _dir, uri, content, position) =
        open_service("        $em->createQuery('SELECT u FROM App\\Entity\\Us§er u');").await;
    let edit = crate::common::rename(&backend, &uri, position.line, position.character, "Member")
        .await
        .expect("no rename edit");
    let renamed = crate::common::apply_edits(&content, &crate::common::edits_for_uri(&edit, &uri));
    assert!(
        renamed.contains("'SELECT u FROM App\\Entity\\Member u'"),
        "{renamed}"
    );
}

#[tokio::test]
async fn an_entity_name_in_dql_is_not_offered_an_import() {
    let (backend, _dir, uri, content, position) =
        open_service("        $em->createQuery('SELECT u FROM App\\Entity\\Us§er u');").await;
    let actions = crate::common::get_code_actions_at(
        &backend,
        uri.as_str(),
        &content,
        position.line,
        position.character,
    );
    assert!(
        crate::common::find_action_containing(&actions, "Import").is_none(),
        "{actions:?}"
    );
}

/// The DQL syntax errors reported for a service whose method body is
/// `body`: the text each covers and its message.
async fn syntax_errors(body: &str) -> Vec<(String, String)> {
    let (backend, _dir, uri, content, _) = open_service(&format!("{body}§")).await;
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), &content, &mut out);
    let lines: Vec<&str> = content.lines().collect();
    out.iter()
        .filter(|d| d.code == Some(Code::String("dql_syntax_error".into())))
        .map(|d| {
            let line = lines[d.range.start.line as usize];
            let covered = if d.range.start.line == d.range.end.line {
                line[d.range.start.character as usize..d.range.end.character as usize].to_string()
            } else {
                line[d.range.start.character as usize..].to_string()
            };
            (covered, message_text(&d.message).to_string())
        })
        .collect()
}

fn pair(covered: &str, message: &str) -> (String, String) {
    let expected = message
        .strip_prefix("expected ")
        .expect("every case states what was expected");
    (covered.to_string(), format!("DQL expects {expected}"))
}

#[tokio::test]
async fn a_query_that_does_not_parse_is_reported() {
    let errors = syntax_errors(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User');
        $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE u.email = :e)');
        $em->createQuery('SELECT u FROM ' . User::class . ' u WHERE u.email');
        $em->createQuery()->setDQL('SELEC u FROM App\\Entity\\User u');",
    )
    .await;
    assert_eq!(
        errors,
        vec![
            pair(
                "'SELECT u FROM App\\Entity\\User'",
                "expected an identification variable, got end of string"
            ),
            pair(")", "expected end of string, got ')'"),
            pair(
                "'SELECT u FROM ' . User::class . ' u WHERE u.email'",
                "expected =, <, <=, <>, >, >= or !=, got end of string"
            ),
            pair("SELEC", "expected SELECT, UPDATE or DELETE, got 'SELEC'"),
        ]
    );
}

#[tokio::test]
async fn a_query_builder_fragment_that_does_not_parse_is_reported() {
    let errors = syntax_errors(
        "        $qb = $em->createQueryBuilder();
        $qb->select('u.id u.email')->from(User::class, 'u')->leftJoin('u.team', 't', 'WITH', 't.id = = 1');
        $qb->andWhere('u.id = 1)')->andWhere($qb->expr()->isNull('u.email)'));
        $qb->groupBy('u.id,')->having('COUNT(u.id > 1')->orderBy('u.email DESC DESC');
        $users->createQueryBuilder('x')->where(['x.id IN :ids']);",
    )
    .await;
    assert_eq!(
        errors,
        vec![
            pair(".", "expected end of string, got '.'"),
            pair("=", "expected a literal, got '='"),
            pair(")", "expected end of string, got ')'"),
            pair(")", "expected end of string, got ')'"),
            pair(
                "'u.id,'",
                "expected an identification variable, got end of string"
            ),
            pair("'COUNT(u.id > 1'", "expected ')', got end of string"),
            pair("DESC", "expected end of string, got 'DESC'"),
            pair(":ids", "expected '(', got ':ids'"),
        ]
    );
}

#[tokio::test]
async fn an_expression_object_that_does_not_parse_is_reported() {
    let errors = syntax_errors(
        "        $em->createQueryBuilder()->select('u')->from(User::class, 'u')
            ->add('where', new \\Doctrine\\ORM\\Query\\Expr\\Andx(['u.id = 1)']))
            ->add('orderBy', new \\Doctrine\\ORM\\Query\\Expr\\OrderBy(sort: 'u.email)'));
        $other = new \\App\\Entity\\OrderBy('u.email)');",
    )
    .await;
    assert_eq!(
        errors,
        vec![
            pair(")", "expected end of string, got ')'"),
            pair(")", "expected end of string, got ')'"),
        ]
    );
}

#[tokio::test]
async fn dql_that_parses_is_not_reported() {
    let errors = syntax_errors(
        "        $em->createQuery('SELECT u, t FROM App\\Entity\\User u LEFT JOIN u.team t WITH t.name LIKE :n WHERE u.id IN (SELECT u2.id FROM App\\Entity\\User u2) AND u.email IS NOT NULL ORDER BY u.email DESC');
        $em->createQuery(\"UPDATE App\\\\Entity\\\\User u SET u.email = 'x' WHERE u.id = ?1\");
        $qb = $em->createQueryBuilder();
        $qb->select('u', 'COUNT(t.id) AS HIDDEN teams')->from(User::class, 'u', 'u.id')->join('u.team', 't', 'WITH', 't.id > 0');
        $qb->where($qb->expr()->orX('u.id = 1', $qb->expr()->in('u.id', 'SELECT u3.id FROM App\\Entity\\User u3')));
        $qb->andWhere($qb->expr()->exists('SELECT t2 FROM App\\Entity\\Team t2'));
        $qb->groupBy('u.id')->having('COUNT(t.id) > 1')->orderBy('u.email', 'ASC')->set('u.email', ':e');
        $qb->andWhere('DATE_FORMAT(u.created, :format) = :day');",
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn dql_whose_call_is_not_doctrine_s_or_whose_text_is_dynamic_is_not_reported() {
    let errors = syntax_errors(
        "        $sql = new \\Doctrine\\DBAL\\Query\\QueryBuilder();
        $sql->select('u.*')->from('users', 'u')->where('u.flags & 4 = 4');
        $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE u.id = ' . $id . ')');
        $em->createQueryBuilder()->select('u')->from(User::class, 'u')->where('u.id IN (' . implode(',', $ids));",
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}

/// The undeclared DQL variables reported for a service whose method body
/// is `body`: the text each covers and its message.
async fn undeclared(body: &str) -> Vec<(String, String)> {
    let opened = open_service(&format!("{body}§")).await;
    undeclared_in(opened).await
}

/// [`undeclared`] for a service whose members are `members`, beside the
/// files `extra`.
async fn undeclared_with(members: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let opened = open_service_with(&format!("{members}§"), extra).await;
    undeclared_in(opened).await
}

async fn undeclared_in(
    (backend, _dir, uri, content, _): (Backend, tempfile::TempDir, Uri, String, Position),
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), &content, &mut out);
    let lines: Vec<&str> = content.lines().collect();
    out.iter()
        .filter(|d| d.code == Some(Code::String("unknown_dql_variable".into())))
        .map(|d| {
            let line = lines[d.range.start.line as usize];
            let covered = &line[d.range.start.character as usize..d.range.end.character as usize];
            (covered.to_string(), message_text(&d.message).to_string())
        })
        .collect()
}

fn undefined(alias: &str) -> (String, String) {
    (
        alias.to_string(),
        format!("Undefined DQL variable '{alias}'"),
    )
}

#[tokio::test]
async fn a_variable_nothing_declares_is_reported_once() {
    let errors = undeclared(
        "        $em->createQuery('SELECT u FROM App\\Entity\\User u WHERE x.id = 1 AND x.email = 2');
        $qb = $em->createQueryBuilder();
        $qb->select('u')->from(User::class, 'u')->where('p.id = 1');
        $qb = $qb->andWhere('p.email = 1');
        $users->createQueryBuilder('r')->join('r.team', 't')->where('t.id = 1 AND q.id = 2');
        $qb->getQuery();",
    )
    .await;
    assert_eq!(errors, vec![undefined("x"), undefined("p"), undefined("q")]);
}

#[tokio::test]
async fn a_variable_a_builder_may_have_been_given_elsewhere_is_not_reported() {
    // Passed to another method, which may join anything onto it.
    let errors = undeclared(
        "        $qb = $em->createQueryBuilder();
        $this->joinTeams($qb);
        $qb->select('u')->from(User::class, 'u')->where('j.id = 1');",
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");

    // Declared under a name the text does not spell.
    let errors = undeclared(
        "        $em->createQueryBuilder()->select('u')->from(User::class, 'u')->join('u.team', $alias)->where('k.id = 1');
        $em->createQuery('SELECT u FROM App\\Entity\\User ' . $alias . ' WHERE v.id = 1');",
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn a_variable_one_path_never_declares_is_reported() {
    // Joined on one branch only.
    let errors = undeclared(
        "        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u')->andWhere('t.id = 1');
        if ($withTeam) {
            $qb->join('u.team', 't');
        }
        $qb->getQuery();",
    )
    .await;
    assert_eq!(errors, vec![undefined("t")]);

    // Used on a branch that does not join it, after one that does.
    let errors = undeclared(
        "        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        switch ($mode) {
            case 1:
                $qb->join('u.team', 't')->andWhere('t.id = 1');
                break;
            default:
                $qb->andWhere('t.id = 2');
        }
        return $qb->getQuery();",
    )
    .await;
    assert_eq!(errors, vec![undefined("t")]);

    // Joined only by a loop that may not run.
    let errors = undeclared(
        "        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u')->andWhere('t.id = 1');
        foreach ($ids as $id) {
            $qb->join('u.team', 't');
        }
        $qb->getQuery();",
    )
    .await;
    assert_eq!(errors, vec![undefined("t")]);
}

#[tokio::test]
async fn a_variable_every_path_to_the_query_declares_is_not_reported() {
    // Declared and used on the same branch, and on both branches.
    let errors = undeclared(
        "        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        if ($withTeam) {
            $qb->join('u.team', 't')->andWhere('t.id = 1');
        }
        if ($a) {
            $qb->join('u.team', 's');
        } elseif ($b) {
            $qb->leftJoin('u.team', 's');
        } else {
            $qb->innerJoin('u.team', 's');
        }
        $qb->andWhere('s.id = 1');
        $qb->getQuery();",
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");

    // The paths that do not declare it throw or return before using it.
    let errors = undeclared(
        "        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        if ($withTeam) {
            $qb->join('u.team', 't');
        } else {
            throw new \\RuntimeException();
        }
        try {
            $qb->join('u.team', 'r');
        } finally {
            $qb->andWhere('r.id = 1');
        }
        do {
            $qb->join('u.team', 'd');
        } while ($more);
        while (true) {
            if ($done) {
                $qb->join('u.team', 'w');
                break;
            }
        }
        if (!$withTeam) {
            return $qb->getQuery();
        }
        $qb->andWhere('t.id = 1 AND d.id = 1');
        return $qb->getQuery();",
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn a_variable_a_builder_another_method_returns_declares_is_not_reported() {
    let errors = undeclared_with(
        "    private EntityManagerInterface $em;

    private function createQb(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('e')->from(User::class, 'e');
    }

    public function run(): void
    {
        $qb = $this->createQb();
        $qb->andWhere('t.id = 1 AND e.id = 1')->getQuery();
        $this->createQb()->andWhere('e.id = 2 AND s.id = 1')->getQuery();
    }",
        &[],
    )
    .await;
    assert_eq!(errors, vec![undefined("t"), undefined("s")]);
}

#[tokio::test]
async fn a_variable_a_method_the_builder_is_passed_to_declares_is_not_reported() {
    let errors = undeclared_with(
        "    private function joinTeams(QueryBuilder $qb): void
    {
        $qb->join('u.team', 't');
    }

    public function run(EntityManagerInterface $em): void
    {
        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $this->joinTeams($qb);
        $qb->andWhere('t.id = 1 AND p.id = 1')->getQuery();
    }",
        &[],
    )
    .await;
    assert_eq!(errors, vec![undefined("p")]);
}

#[tokio::test]
async fn a_variable_used_on_a_builder_a_method_returns_is_left_to_its_caller() {
    // The caller may declare it, so the method that returns the builder
    // does not report it.
    let errors = undeclared_with(
        "    private EntityManagerInterface $em;

    private function base(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('u')->from(User::class, 'u')->andWhere('t.id = 1');
    }

    private function filtered(): QueryBuilder
    {
        $qb = $this->em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $qb->andWhere('r.id = 1');
        return $qb;
    }

    public function run(): void
    {
        $this->base()->join('u.team', 't')->getQuery();
        $this->filtered()->join('u.team', 'r')->getQuery();
    }",
        &[],
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn a_variable_a_method_out_of_sight_may_declare_is_not_reported() {
    let errors = undeclared_with(
        "    private QueryBuilder $qb;
    private EntityManagerInterface $em;

    private function joinAny(QueryBuilder $qb, string $alias): void
    {
        $qb->join('u.team', $alias);
    }

    private function joinElsewhere(QueryBuilder $qb): void
    {
        $qb->join('u.team', 't');
        $this->joinAny($qb, 'x');
    }

    private function stored(): QueryBuilder
    {
        return $this->qb;
    }

    private function wrapped(): QueryBuilder
    {
        return $this->createOther();
    }

    public function opaque(): void
    {
        $qb = $this->em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $this->joinAny($qb, 'a');
        $qb->andWhere('a.id = 1')->getQuery();
    }

    public function passedOn(): void
    {
        $qb = $this->em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $this->joinElsewhere($qb);
        $qb->andWhere('x.id = 1')->getQuery();
    }

    public function fromProperty(): void
    {
        $this->stored()->andWhere('b.id = 1')->getQuery();
    }

    public function fromAnotherMethod(): void
    {
        $this->wrapped()->andWhere('c.id = 1')->getQuery();
    }

    public function fromNowhere(): void
    {
        $this->missing()->andWhere('d.id = 1')->getQuery();
    }",
        &[],
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn a_variable_an_inherited_method_s_builder_declares_is_read_from_its_file() {
    let errors = undeclared_with(
        "    public function run(): void
    {
        $this->createQb()->andWhere('e.id = 1 AND t.id = 1')->getQuery();
    }",
        &[(
            "src/Service/BaseUsers.php",
            "<?php
namespace App\\Service;
use App\\Entity\\User;
use Doctrine\\ORM\\EntityManagerInterface;
use Doctrine\\ORM\\QueryBuilder;
abstract class BaseUsers {
    protected EntityManagerInterface $em;
    protected function createQb(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('e')->from(User::class, 'e');
    }
}
",
        )],
    )
    .await;
    assert_eq!(errors, vec![undefined("t")]);
}

#[tokio::test]
async fn a_variable_a_chain_of_methods_declares_is_not_reported() {
    // The builder comes from a method that gets it from another, and is
    // passed to a method that passes it on to another, which passes it back.
    let errors = undeclared_with(
        "    private EntityManagerInterface $em;

    private function createQb(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('e')->from(User::class, 'e');
    }

    private function base(): QueryBuilder
    {
        return $this->createQb()->join('e.team', 't');
    }

    private function filtered(): QueryBuilder
    {
        $qb = $this->base();
        $qb->andWhere('t.id = 1');
        return $qb;
    }

    private function joinTeams(QueryBuilder $qb, bool $again): void
    {
        $qb->join('u.team', 's');
        $this->joinOwner($qb, $again);
    }

    private function joinOwner(QueryBuilder $qb, bool $again): void
    {
        $qb->join('s.owner', 'o');
        if ($again) {
            $this->joinTeams($qb, false);
        }
    }

    public function fromAChain(): void
    {
        $this->filtered()->andWhere('e.id = 1 AND t.id = 1 AND x.id = 1')->getQuery();
    }

    public function passedOn(): void
    {
        $qb = $this->em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $this->joinTeams($qb, true);
        $qb->andWhere('s.id = 1 AND o.id = 1 AND p.id = 1')->getQuery();
    }",
        &[],
    )
    .await;
    assert_eq!(errors, vec![undefined("x"), undefined("p")]);
}

#[tokio::test]
async fn a_variable_a_static_call_or_another_object_s_builder_declares_is_not_reported() {
    let errors = undeclared_with(
        "    private static function createQb(EntityManagerInterface $em): QueryBuilder
    {
        return $em->createQueryBuilder()->select('e')->from(User::class, 'e');
    }

    private static function joinTeams(QueryBuilder $qb): void
    {
        $qb->join('e.team', 't');
    }

    public function run(EntityManagerInterface $em, Builders $builders): void
    {
        $qb = self::createQb($em);
        static::joinTeams($qb);
        $qb->andWhere('e.id = 1 AND t.id = 1 AND x.id = 1')->getQuery();
        $builders->forTeams()->andWhere('g.id = 1 AND m.id = 1 AND y.id = 1')->getQuery();
    }",
        &[(
            "src/Service/Builders.php",
            "<?php
namespace App\\Service;
use App\\Entity\\Team;
use Doctrine\\ORM\\EntityManagerInterface;
use Doctrine\\ORM\\QueryBuilder;
final class Builders {
    private EntityManagerInterface $em;
    private function base(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('g')->from(Team::class, 'g');
    }
    public function forTeams(): QueryBuilder
    {
        $qb = $this->base();
        $this->joinMembers($qb);
        return $qb;
    }
    private function joinMembers(QueryBuilder $qb): void
    {
        $qb->join('g.members', 'm');
    }
}
",
        )],
    )
    .await;
    assert_eq!(errors, vec![undefined("x"), undefined("y")]);
}

#[tokio::test]
async fn a_variable_another_method_declares_counts_only_on_the_paths_that_call_it() {
    let errors = undeclared_with(
        "    private function joinTeams(QueryBuilder $qb): void
    {
        $qb->join('u.team', 't');
    }

    public function sometimes(EntityManagerInterface $em, bool $withTeam): void
    {
        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        if ($withTeam) {
            $this->joinTeams($qb);
        }
        $qb->andWhere('t.id = 1')->getQuery();
    }

    public function laterOnly(EntityManagerInterface $em, bool $withTeam): void
    {
        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        if ($withTeam) {
            $this->joinTeams($qb);
            $qb->andWhere('t.id = 1');
        } else {
            $qb->andWhere('t.id = 2');
        }
        $qb->getQuery();
    }

    public function always(EntityManagerInterface $em, bool $withTeam): void
    {
        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        if ($withTeam) {
            $this->joinTeams($qb);
        } else {
            $qb->join('u.team', 't');
        }
        $qb->andWhere('t.id = 3')->getQuery();
    }",
        &[],
    )
    .await;
    assert_eq!(errors, vec![undefined("t"), undefined("t")]);
}

fn undefined_in_run(alias: &str, method: &str) -> (String, String) {
    (
        "getQuery".to_string(),
        format!("Undefined DQL variable '{alias}' in {method}()"),
    )
}

#[tokio::test]
async fn a_variable_a_returned_builder_uses_is_reported_where_no_caller_declares_it() {
    let errors = undeclared_with(
        "    private EntityManagerInterface $em;

    private function base(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('u')->from(User::class, 'u')->andWhere('t.id = 1');
    }

    private function filtered(): QueryBuilder
    {
        $qb = $this->em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $qb->andWhere('r.id = 1');
        return $qb;
    }

    private function wrapped(): QueryBuilder
    {
        return $this->filtered()->andWhere('w.id = 1');
    }

    public function chained(): void
    {
        $this->base()->getQuery();
    }

    public function stored(): void
    {
        $qb = $this->filtered();
        $qb->andWhere('u.id = 1');
        $qb->getQuery();
    }

    public function throughAnother(): void
    {
        $this->wrapped()->join('u.team', 'r')->getQuery();
    }

    public function declared(): void
    {
        $this->base()->join('u.team', 't')->getQuery();
    }

    public function sometimes(bool $withTeam): void
    {
        $qb = $this->base();
        if ($withTeam) {
            $qb->join('u.team', 't');
        }
        $qb->getQuery();
    }",
        &[],
    )
    .await;
    assert_eq!(
        errors,
        vec![
            undefined_in_run("t", "base"),
            undefined_in_run("r", "filtered"),
            undefined_in_run("w", "wrapped"),
            undefined_in_run("t", "base"),
        ]
    );
}

#[tokio::test]
async fn a_variable_a_method_the_builder_is_passed_to_uses_is_reported_where_the_query_runs() {
    let errors = undeclared_with(
        "    private function onlyActive(QueryBuilder $qb): void
    {
        $qb->andWhere('t.active = 1');
    }

    private function joinTeams(QueryBuilder $qb): void
    {
        $qb->join('u.team', 't');
    }

    public function bare(EntityManagerInterface $em): void
    {
        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $this->onlyActive($qb);
        $qb->getQuery();
    }

    public function joinedByAnother(EntityManagerInterface $em): void
    {
        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $this->joinTeams($qb);
        $this->onlyActive($qb);
        $qb->getQuery();
    }

    public function joinedHere(EntityManagerInterface $em): void
    {
        $qb = $em->createQueryBuilder()->select('u')->from(User::class, 'u');
        $this->onlyActive($qb);
        $qb->join('u.team', 't')->getQuery();
    }",
        &[],
    )
    .await;
    assert_eq!(errors, vec![undefined_in_run("t", "onlyActive")]);
}

#[tokio::test]
async fn a_variable_a_returned_builder_uses_is_left_to_a_caller_out_of_sight() {
    let errors = undeclared_with(
        "    private EntityManagerInterface $em;
    private QueryBuilder $kept;

    private function base(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('u')->from(User::class, 'u')->andWhere('t.id = 1');
    }

    private function joinAny(QueryBuilder $qb, string $alias): void
    {
        $qb->join('u.team', $alias);
    }

    public function returned(): QueryBuilder
    {
        $qb = $this->base();
        $qb->getQuery();
        return $qb;
    }

    public function stored(): void
    {
        $qb = $this->base();
        $this->kept = $qb;
        $qb->getQuery();
    }

    public function opaque(): void
    {
        $qb = $this->base();
        $this->joinAny($qb, 't');
        $qb->getQuery();
    }

    public function fromNowhere(): void
    {
        $this->missing()->getQuery();
    }",
        &[],
    )
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}

#[tokio::test]
async fn a_variable_a_builder_from_another_file_uses_is_reported_where_the_query_runs() {
    let errors = undeclared_with(
        "    public function run(Builders $builders): void
    {
        $builders->forTeams()->getQuery();
    }",
        &[(
            "src/Service/Builders.php",
            "<?php
namespace App\\Service;
use App\\Entity\\Team;
use Doctrine\\ORM\\EntityManagerInterface;
use Doctrine\\ORM\\QueryBuilder;
final class Builders {
    private EntityManagerInterface $em;
    private function base(): QueryBuilder
    {
        return $this->em->createQueryBuilder()->select('g')->from(Team::class, 'g');
    }
    public function forTeams(): QueryBuilder
    {
        $qb = $this->base();
        $this->onlyActive($qb);
        return $qb;
    }
    private function onlyActive(QueryBuilder $qb): void
    {
        $qb->andWhere('m.active = 1');
    }
}
",
        )],
    )
    .await;
    assert_eq!(errors, vec![undefined_in_run("m", "forTeams")]);
}
