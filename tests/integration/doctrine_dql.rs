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
    let source = format!(
        "<?php
namespace App\\Service;

use App\\Entity\\Team;
use App\\Entity\\User;
use App\\Repository\\UserRepository;
use Doctrine\\ORM\\EntityManagerInterface;

final class Users
{{
    public function run(UserRepository $users, EntityManagerInterface $em): void
    {{
{body}
    }}
}}
"
    );
    let (stripped, position) = split_cursor(&source);
    let path = "src/Service/Users.php";
    let mut files: Vec<(&str, &str)> = STUBS.to_vec();
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
    format!("Field '{field}' is not mapped on entity 'App\\Entity\\{entity}'")
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
            "Class 'App\\Entity\\Missing' not found".to_string(),
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
    (covered.to_string(), format!("DQL syntax error: {message}"))
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
    let (backend, _dir, uri, content, _) = open_service(&format!("{body}§")).await;
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
