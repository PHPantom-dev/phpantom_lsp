//! The entity fields a Doctrine repository call names: the magic
//! `findBy<Field>()`, `findOneBy<Field>()`, and `countBy<Field>()` finders,
//! and the field names `findBy()`, `findOneBy()`, and `count()` take as the
//! keys of their criteria and order.

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
        "Doctrine\\ORM\\": "stubs/orm/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[
    (
        "stubs/orm/EntityRepository.php",
        "<?php
namespace Doctrine\\ORM;
/** @template T of object */
class EntityRepository {
    /** @return list<T> */
    public function findBy(array $criteria, ?array $orderBy = null, ?int $limit = null, ?int $offset = null): array {}
    /** @return T|null */
    public function findOneBy(array $criteria, ?array $orderBy = null): ?object {}
    public function count(array $criteria = []): int {}
    public function __call(string $method, array $arguments): mixed {}
}
",
    ),
    (
        "stubs/orm/EntityManagerInterface.php",
        "<?php
namespace Doctrine\\ORM;
interface EntityManagerInterface {
    /**
     * @template T of object
     * @param class-string<T> $className
     * @return EntityRepository<T>
     */
    public function getRepository(string $className): EntityRepository;
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
        "src/Entity/Timestamps.php",
        "<?php
namespace App\\Entity;
use Doctrine\\ORM\\Mapping as ORM;
trait Timestamps {
    #[ORM\\Column(type: 'datetime')]
    private \\DateTimeInterface $createdAt;
}
",
    ),
    (
        "src/Entity/User.php",
        "<?php
namespace App\\Entity;
use App\\Repository\\UserRepository;
use Doctrine\\ORM\\Mapping as ORM;
#[ORM\\Entity(repositoryClass: UserRepository::class)]
class User {
    use Timestamps;
    #[ORM\\Id]
    #[ORM\\Column]
    private int $id;
    #[ORM\\Column]
    private string $email;
    #[ORM\\ManyToOne(targetEntity: Team::class)]
    private Team $team;
    private string $plainPassword;
    public function __construct(#[ORM\\Column] private string $displayName) {}
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
}
",
    ),
    (
        "src/Entity/Order.php",
        "<?php\nnamespace App\\Entity;\nclass Order {\n    private string $reference;\n}\n",
    ),
    (
        "src/Entity/Legacy.php",
        "<?php
namespace App\\Entity;
/** @ORM\\Entity */
class Legacy {
    /** @ORM\\Column */
    private string $code;
}
",
    ),
    (
        "src/Repository/OrderRepository.php",
        "<?php
namespace App\\Repository;
/** @extends \\Doctrine\\ORM\\EntityRepository<\\App\\Entity\\Order> */
class OrderRepository extends \\Doctrine\\ORM\\EntityRepository {}
",
    ),
    (
        "src/Repository/UserRepository.php",
        "<?php
namespace App\\Repository;
use App\\Entity\\User;
/** @extends \\Doctrine\\ORM\\EntityRepository<User> */
class UserRepository extends \\Doctrine\\ORM\\EntityRepository {
    public function findActive(): array { return []; }
}
",
    ),
];

const ORDER_MAPPING_PATH: &str = "config/doctrine/Order.orm.xml";

const ORDER_MAPPING: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<doctrine-mapping>
  <entity name="App\Entity\Order">
    <id name="id" type="integer"/>
    <field name="reference" type="string"/>
  </entity>
</doctrine-mapping>
"#;

/// A workspace with the Doctrine stubs and entities, and a service whose
/// method body is `body`, opened at the `§` marker.
async fn open_service(body: &str) -> (Backend, tempfile::TempDir, Uri, String, Position) {
    let source = format!(
        "<?php
namespace App\\Service;

use App\\Entity\\Legacy;
use App\\Entity\\Order;
use App\\Entity\\Team;
use App\\Repository\\OrderRepository;
use App\\Repository\\UserRepository;
use Doctrine\\ORM\\EntityManagerInterface;

final class Users
{{
    public function run(UserRepository $users, OrderRepository $orders, EntityManagerInterface $em): void
    {{
{body}
    }}
}}
"
    );
    let (stripped, position) = split_cursor(&source);
    let path = "src/Service/Users.php";
    let mut files: Vec<(&str, &str)> = STUBS.to_vec();
    files.push((ORDER_MAPPING_PATH, ORDER_MAPPING));
    files.push((path, stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
    let mapping_uri = Uri::from_file_path(dir.path().join(ORDER_MAPPING_PATH)).unwrap();
    open_document(&backend, &mapping_uri, "xml", ORDER_MAPPING).await;
    let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
    open_document(&backend, &uri, "php", &stripped).await;
    (backend, dir, uri, stripped, position)
}

async fn completions(body: &str) -> Vec<String> {
    let (backend, _dir, uri, _, position) = open_service(body).await;
    complete_labels_at_opened(&backend, &uri, position.line, position.character).await
}

/// The unknown-member and unknown-field messages for a service whose
/// method body is `body`.
async fn diagnostics(body: &str) -> Vec<String> {
    let (backend, _dir, uri, content, _) = open_service(&format!("{body}§")).await;
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri.as_str(), &content, &mut out);
    out.iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn a_repository_answers_a_finder_for_each_mapped_field() {
    let labels = completions("        $users->§").await;
    for finder in [
        "findByEmail",
        "findOneByEmail",
        "countByEmail",
        "findByTeam",
        "findById",
        "findByDisplayName",
        "findByCreatedAt",
    ] {
        assert!(
            labels.iter().any(|l| l.starts_with(finder)),
            "{finder} missing from {labels:?}"
        );
    }
    assert!(
        !labels.iter().any(|l| l.starts_with("findByPlainPassword")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn a_finder_returns_the_entity() {
    let (backend, _dir, uri, _, position) = open_service(
        "        $found = $users->findOneByEmail('ada@example.com');\n        $fo§und;",
    )
    .await;
    let hover = hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("$found = ?User"), "{hover}");
}

#[tokio::test]
async fn a_repository_bound_by_its_type_argument_answers_finders() {
    let labels = completions("        $em->getRepository(Team::class)->§").await;
    assert!(
        labels.iter().any(|l| l.starts_with("findByName")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn an_xml_mapping_maps_the_fields() {
    let labels = completions("        $orders->§").await;
    assert!(
        labels.iter().any(|l| l.starts_with("findByReference")),
        "{labels:?}"
    );
}

/// A mapping file nobody opened is read at startup, before the background
/// workspace index reaches the resource files.
#[tokio::test]
async fn a_mapping_file_is_read_at_startup() {
    use tower_lsp_server::LanguageServer;

    let source = "<?php
namespace App\\Service;

use App\\Repository\\OrderRepository;

final class Orders
{
    public function run(OrderRepository $orders): void
    {
        $orders->§
    }
}
";
    let (stripped, position) = split_cursor(source);
    let path = "src/Service/Orders.php";
    let mut files: Vec<(&str, &str)> = STUBS.to_vec();
    files.push((ORDER_MAPPING_PATH, ORDER_MAPPING));
    files.push((path, stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
    open_document(&backend, &uri, "php", &stripped).await;
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    assert!(
        labels.iter().any(|l| l.starts_with("findByReference")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn a_finder_for_an_unmapped_field_is_reported() {
    let messages = diagnostics(
        "        $users->findByEmail('a');\n        $users->findByPlainPassword('a');\n        $users->findActive();\n        $orders->findByReference('x');\n        $orders->findByTotal(1);\n        $em->getRepository(Team::class)->findByName('a');\n        $em->getRepository(Team::class)->findByTotal(1);",
    )
    .await;
    assert_eq!(
        messages,
        vec![
            "Method App\\Repository\\UserRepository::findByPlainPassword() not found".to_string(),
            "Method App\\Repository\\OrderRepository::findByTotal() not found".to_string(),
            "Method Doctrine\\ORM\\EntityRepository::findByTotal() not found".to_string(),
        ]
    );
}

#[tokio::test]
async fn an_entity_whose_mapping_is_not_read_is_left_alone() {
    let messages =
        diagnostics("        $legacy = $em->getRepository(Legacy::class);\n        $legacy->findByAnything('a');\n        $legacy->findBy(['anything' => 1]);")
            .await;
    assert!(messages.is_empty(), "{messages:?}");
}

#[tokio::test]
async fn an_unmapped_criteria_key_is_reported() {
    let messages = diagnostics(
        "        $users->findBy(['email' => 'a', 'plainPassword' => 'b'], ['createdAt' => 'DESC', 'nope' => 'ASC']);\n        $users->count(['team' => null]);\n        $orders->findOneBy(criteria: ['reference' => 'x', 'total' => 1]);\n        $em->getRepository(Team::class)->count(['name' => 'a', 'size' => 1]);\n        $data = ['other' => 1];",
    )
    .await;
    assert_eq!(
        messages,
        vec![
            "Field 'plainPassword' is not mapped on entity App\\Entity\\User".to_string(),
            "Field 'nope' is not mapped on entity App\\Entity\\User".to_string(),
            "Field 'total' is not mapped on entity App\\Entity\\Order".to_string(),
            "Field 'size' is not mapped on entity App\\Entity\\Team".to_string(),
        ]
    );
}

#[tokio::test]
async fn criteria_keys_complete_the_mapped_fields() {
    let labels = completions("        $users->findBy(['email' => 'a', '§']);").await;
    assert!(labels.contains(&"team".to_string()), "{labels:?}");
    assert!(labels.contains(&"displayName".to_string()), "{labels:?}");
    assert!(!labels.contains(&"email".to_string()), "{labels:?}");
    assert!(!labels.contains(&"plainPassword".to_string()), "{labels:?}");
}

#[tokio::test]
async fn order_keys_complete_the_mapped_fields() {
    let labels = completions("        $users->findOneBy([], ['cre§']);").await;
    assert_eq!(labels, vec!["createdAt".to_string()]);
}

#[tokio::test]
async fn a_criteria_key_goes_to_the_property() {
    let (backend, dir, uri, _, position) =
        open_service("        $users->findBy(['em§ail' => 'a']);").await;
    let Some(DefinitionResponse::Definition(Definition::Location(location))) =
        goto_definition_at(&backend, &uri, position.line, position.character).await
    else {
        panic!("no definition");
    };
    let user = Uri::from_file_path(dir.path().join("src/Entity/User.php")).unwrap();
    assert_eq!(location.uri, user);
    assert_eq!(location.range.start.line, 11);
}

#[tokio::test]
async fn a_criteria_key_shows_the_property_on_hover() {
    let (backend, _dir, uri, _, position) =
        open_service("        $users->findBy(['em§ail' => 'a']);").await;
    let hover = hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("$email"), "{hover}");
}

#[tokio::test]
async fn a_criteria_key_is_a_reference_to_the_property() {
    let (backend, _dir, uri, _, position) =
        open_service("        $users->findBy(['em§ail' => 'a']);").await;
    let locations = find_references(&backend, &uri, position, false).await;
    assert!(
        locations.iter().any(|l| l.uri == uri),
        "the key itself is missing from {locations:?}"
    );
}

#[tokio::test]
async fn a_key_on_a_repository_bound_by_its_type_argument_finds_the_property() {
    let (backend, dir, uri, _, position) = open_service(
        "        $em->getRepository(Team::class)->findBy(['na§me' => 'a']);\n        $em->getRepository(Team::class)->count(['name' => 'b']);",
    )
    .await;
    let locations = find_references(&backend, &uri, position, true).await;
    let team = Uri::from_file_path(dir.path().join("src/Entity/Team.php")).unwrap();
    assert!(
        locations.iter().any(|l| l.uri == team),
        "the property is missing from {locations:?}"
    );
    assert_eq!(
        locations.iter().filter(|l| l.uri == uri).count(),
        2,
        "{locations:?}"
    );
}

#[tokio::test]
async fn renaming_the_property_from_a_key_renames_the_keys() {
    let (backend, dir, uri, content, position) =
        open_service("        $users->findBy(['em§ail' => 'a'], ['email' => 'ASC']);").await;
    let edit = crate::common::rename(&backend, &uri, position.line, position.character, "mail")
        .await
        .expect("no rename edit");
    let keys = crate::common::edits_for_uri(&edit, &uri);
    assert_eq!(keys.len(), 2, "{keys:?}");
    for key in &keys {
        assert_eq!(key.new_text, "mail");
        let line = content.lines().nth(key.range.start.line as usize).unwrap();
        let start = key.range.start.character as usize;
        let end = key.range.end.character as usize;
        assert_eq!(&line[start..end], "email", "{keys:?}");
    }
    let user = Uri::from_file_path(dir.path().join("src/Entity/User.php")).unwrap();
    assert!(
        !crate::common::edits_for_uri(&edit, &user).is_empty(),
        "the property is not renamed: {edit:?}"
    );
}
