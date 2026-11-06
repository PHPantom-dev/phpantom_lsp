//! The repository Doctrine's `getRepository()` returns: the one the
//! entity's `#[ORM\Entity(repositoryClass: ...)]` attribute or its YAML/XML
//! mapping names, rather than the default repository the method declares.

use crate::common::{
    complete_labels_at_opened, create_psr4_workspace, hover_text_at, open_document, split_cursor,
};
use phpantom_lsp::Backend;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "doctrine/orm": "^3.0" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Doctrine\\ORM\\": "stubs/orm/",
        "Doctrine\\Persistence\\": "stubs/persistence/"
    } }
}"#;

/// Just enough of Doctrine for the declared `getRepository()` types.
const STUBS: &[(&str, &str)] = &[
    (
        "stubs/persistence/ObjectRepository.php",
        "<?php\nnamespace Doctrine\\Persistence;\n/** @template T of object */\ninterface ObjectRepository {\n    /** @return T|null */\n    public function find(mixed $id): ?object;\n}\n",
    ),
    (
        "stubs/persistence/ObjectManager.php",
        "<?php\nnamespace Doctrine\\Persistence;\ninterface ObjectManager {\n    /**\n     * @template T of object\n     * @param class-string<T> $className\n     * @return ObjectRepository<T>\n     */\n    public function getRepository(string $className): ObjectRepository;\n}\n",
    ),
    (
        "stubs/persistence/ManagerRegistry.php",
        "<?php\nnamespace Doctrine\\Persistence;\ninterface ManagerRegistry {\n    /**\n     * @template T of object\n     * @param class-string<T> $persistentObject\n     * @return ObjectRepository<T>\n     */\n    public function getRepository(string $persistentObject, ?string $persistentManagerName = null): ObjectRepository;\n}\n",
    ),
    (
        "stubs/orm/EntityRepository.php",
        "<?php\nnamespace Doctrine\\ORM;\n/**\n * @template T of object\n * @implements \\Doctrine\\Persistence\\ObjectRepository<T>\n */\nclass EntityRepository implements \\Doctrine\\Persistence\\ObjectRepository {\n    /** @return T|null */\n    public function find(mixed $id): ?object {}\n}\n",
    ),
    (
        "stubs/orm/EntityManagerInterface.php",
        "<?php\nnamespace Doctrine\\ORM;\ninterface EntityManagerInterface extends \\Doctrine\\Persistence\\ObjectManager {\n    /**\n     * @template T of object\n     * @param class-string<T> $className\n     * @return EntityRepository<T>\n     */\n    public function getRepository(string $className): EntityRepository;\n}\n",
    ),
    (
        "stubs/orm/Mapping/Entity.php",
        "<?php\nnamespace Doctrine\\ORM\\Mapping;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nfinal class Entity {\n    public function __construct(public readonly ?string $repositoryClass = null, public readonly bool $readOnly = false) {}\n}\n",
    ),
    (
        "src/Entity/User.php",
        "<?php\nnamespace App\\Entity;\nuse App\\Repository\\UserRepository;\nuse Doctrine\\ORM\\Mapping as ORM;\n#[ORM\\Entity(repositoryClass: UserRepository::class)]\nclass User {}\n",
    ),
    (
        "src/Entity/Post.php",
        "<?php\nnamespace App\\Entity;\nuse Doctrine\\ORM\\Mapping\\Entity;\n#[Entity('App\\Repository\\PostRepository')]\nclass Post {}\n",
    ),
    (
        "src/Entity/Comment.php",
        "<?php\nnamespace App\\Entity;\nuse Doctrine\\ORM\\Mapping as ORM;\n#[ORM\\Entity(readOnly: true, repositoryClass: \\App\\Repository\\SharedRepository::class)]\nclass Comment {}\n",
    ),
    (
        "src/Entity/Tag.php",
        "<?php\nnamespace App\\Entity;\nuse Doctrine\\ORM\\Mapping as ORM;\n#[ORM\\Entity(repositoryClass: \\App\\Repository\\SharedRepository::class)]\nclass Tag {}\n",
    ),
    (
        "src/Entity/Plain.php",
        "<?php\nnamespace App\\Entity;\nuse Doctrine\\ORM\\Mapping as ORM;\n#[ORM\\Entity]\nclass Plain {}\n",
    ),
    (
        "src/Entity/Mapped.php",
        "<?php\nnamespace App\\Entity;\nclass Mapped {}\n",
    ),
    (
        "src/Entity/Local.php",
        "<?php\nnamespace App\\Entity;\nuse Doctrine\\ORM\\Mapping as ORM;\n#[ORM\\Entity(repositoryClass: UserRepository::class)]\nclass Local {}\n",
    ),
    (
        "src/Entity/UserRepository.php",
        "<?php\nnamespace App\\Entity;\n/** @extends \\Doctrine\\ORM\\EntityRepository<Local> */\nclass UserRepository extends \\Doctrine\\ORM\\EntityRepository {\n    public function findLocal(): void {}\n}\n",
    ),
    (
        "src/Repository/UserRepository.php",
        "<?php\nnamespace App\\Repository;\nuse App\\Entity\\User;\n/** @extends \\Doctrine\\ORM\\EntityRepository<User> */\nclass UserRepository extends \\Doctrine\\ORM\\EntityRepository {\n    /** @return list<User> */\n    public function findActive(): array {}\n}\n",
    ),
    (
        "src/Repository/PostRepository.php",
        "<?php\nnamespace App\\Repository;\n/** @extends \\Doctrine\\ORM\\EntityRepository<\\App\\Entity\\Post> */\nclass PostRepository extends \\Doctrine\\ORM\\EntityRepository {}\n",
    ),
    (
        "src/Repository/SharedRepository.php",
        "<?php\nnamespace App\\Repository;\n/**\n * @template T of object\n * @extends \\Doctrine\\ORM\\EntityRepository<T>\n */\nclass SharedRepository extends \\Doctrine\\ORM\\EntityRepository {}\n",
    ),
    (
        "src/Repository/MappedRepository.php",
        "<?php\nnamespace App\\Repository;\n/** @extends \\Doctrine\\ORM\\EntityRepository<\\App\\Entity\\Mapped> */\nclass MappedRepository extends \\Doctrine\\ORM\\EntityRepository {}\n",
    ),
];

const MAPPING_YAML: &str =
    "App\\Entity\\Mapped:\n  type: entity\n  repositoryClass: App\\Repository\\MappedRepository\n";

const MAPPING_PATH: &str = "config/doctrine/Mapped.orm.yml";

/// The hover at the `§` marker in a service method whose body is `body`.
async fn hover(body: &str) -> String {
    let (backend, _dir, uri, position) = open_service(body).await;
    hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default()
}

/// The completion labels at the `§` marker in a service method whose body
/// is `body`.
async fn completions(body: &str) -> Vec<String> {
    let (backend, _dir, uri, position) = open_service(body).await;
    complete_labels_at_opened(&backend, &uri, position.line, position.character).await
}

/// A workspace with the Doctrine stubs, the entities and their mapping,
/// and a service whose method body is `body`, opened at the `§` marker.
async fn open_service(body: &str) -> (Backend, tempfile::TempDir, Uri, Position) {
    let source = format!(
        "<?php
namespace App\\Service;

use App\\Entity\\Comment;
use App\\Entity\\Local;
use App\\Entity\\Mapped;
use App\\Entity\\Plain;
use App\\Entity\\Post;
use App\\Entity\\Tag;
use App\\Entity\\User;
use Doctrine\\ORM\\EntityManagerInterface;
use Doctrine\\Persistence\\ManagerRegistry;

final class Users
{{
    /** @param class-string<Comment|Tag> $shared */
    public function run(EntityManagerInterface $em, ManagerRegistry $registry, string $shared): void
    {{
{body}
    }}
}}
"
    );
    let (stripped, position) = split_cursor(&source);
    let path = "src/Service/Users.php";
    let mut files: Vec<(&str, &str)> = STUBS.to_vec();
    files.push((MAPPING_PATH, MAPPING_YAML));
    files.push((path, stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER, &files);
    let mapping_uri = Uri::from_file_path(dir.path().join(MAPPING_PATH)).unwrap();
    open_document(&backend, &mapping_uri, "yaml", MAPPING_YAML).await;
    let uri = Uri::from_file_path(dir.path().join(path)).unwrap();
    open_document(&backend, &uri, "php", &stripped).await;
    (backend, dir, uri, position)
}

/// Whether a variable's hover shows it typed as exactly `ty`.
fn shows(hover: &str, ty: &str) -> bool {
    hover.contains(&format!(" = {ty}\n"))
}

#[tokio::test]
async fn an_attribute_names_the_entity_repository() {
    let hover = hover("        $repo = $em->getRepository(User::class);\n        $re§po;").await;
    assert!(shows(&hover, "UserRepository"), "{hover}");
}

#[tokio::test]
async fn the_repository_own_methods_complete_on_a_chained_call() {
    let labels = completions("        $em->getRepository(User::class)->§").await;
    assert!(
        labels.iter().any(|l| l.starts_with("findActive")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn a_string_names_the_repository_by_its_full_name() {
    let hover = hover("        $repo = $em->getRepository(Post::class);\n        $re§po;").await;
    assert!(shows(&hover, "PostRepository"), "{hover}");
}

#[tokio::test]
async fn an_unimported_repository_name_resolves_in_the_entity_namespace() {
    let labels = completions("        $em->getRepository(Local::class)->§").await;
    assert!(
        labels.iter().any(|l| l.starts_with("findLocal")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("findActive")),
        "{labels:?}"
    );
}

#[tokio::test]
async fn a_yaml_mapping_names_the_entity_repository() {
    let hover = hover("        $repo = $em->getRepository(Mapped::class);\n        $re§po;").await;
    assert!(shows(&hover, "MappedRepository"), "{hover}");
}

#[tokio::test]
async fn a_generic_repository_is_bound_to_the_entity() {
    let hover = hover(
        "        $repo = $registry->getRepository(persistentObject: Comment::class);\n        $re§po;",
    )
    .await;
    assert!(shows(&hover, "SharedRepository<Comment>"), "{hover}");
}

#[tokio::test]
async fn entities_sharing_a_repository_are_bound_together() {
    let hover = hover("        $repo = $em->getRepository($shared);\n        $re§po;").await;
    assert!(shows(&hover, "SharedRepository<Comment|Tag>"), "{hover}");
}

#[tokio::test]
async fn an_entity_without_a_repository_keeps_the_declared_type() {
    let hover = hover("        $repo = $em->getRepository(Plain::class);\n        $re§po;").await;
    assert!(shows(&hover, "EntityRepository<Plain>"), "{hover}");
}
