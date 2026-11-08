//! Drupal's plugins: the plugin types the project's plugin managers declare
//! by their constructors, and the plugin IDs their classes declare, read by
//! a manager's lookups and by the field API's calls that name a plugin.

use crate::common::{completion_response, response_labels};
use crate::drupal_services::{definitions_at, position_in, references_at, uri_for, workspace_in};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

/// Core's plugin API, the managers, and core's plugins, where Composer
/// would put core.
const CORE: &[(&str, &str)] = &[
    (
        "web/core/lib/Drupal/Component/Plugin/PluginManagerInterface.php",
        "<?php\nnamespace Drupal\\Component\\Plugin;\ninterface PluginManagerInterface {\n    public function createInstance($plugin_id, array $configuration = []);\n    public function getDefinition($plugin_id, $exception_on_invalid = TRUE);\n    public function hasDefinition($plugin_id);\n}\n",
    ),
    (
        "web/core/lib/Drupal/Component/Plugin/Attribute/AttributeInterface.php",
        "<?php\nnamespace Drupal\\Component\\Plugin\\Attribute;\ninterface AttributeInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Component/Plugin/Attribute/Plugin.php",
        "<?php\nnamespace Drupal\\Component\\Plugin\\Attribute;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass Plugin implements AttributeInterface {\n    public function __construct(public readonly string $id) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Plugin/DefaultPluginManager.php",
        "<?php\nnamespace Drupal\\Core\\Plugin;\nuse Drupal\\Component\\Plugin\\PluginManagerInterface;\nclass DefaultPluginManager implements PluginManagerInterface {\n    public function __construct($subdir, \\Traversable $namespaces, $module_handler, $plugin_interface = NULL, ?string $plugin_definition_attribute_name = NULL, $plugin_definition_annotation_name = NULL) {}\n    public function createInstance($plugin_id, array $configuration = []) {}\n    public function getDefinition($plugin_id, $exception_on_invalid = TRUE) {}\n    public function hasDefinition($plugin_id) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Block/Attribute/Block.php",
        "<?php\nnamespace Drupal\\Core\\Block\\Attribute;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass Block extends Plugin {\n    public function __construct(public readonly string $id, public readonly ?string $admin_label = NULL) {}\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Block/BlockManagerInterface.php",
        "<?php\nnamespace Drupal\\Core\\Block;\nuse Drupal\\Component\\Plugin\\PluginManagerInterface;\ninterface BlockManagerInterface extends PluginManagerInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Block/BlockManager.php",
        "<?php\nnamespace Drupal\\Core\\Block;\nuse Drupal\\Core\\Block\\Attribute\\Block;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass BlockManager extends DefaultPluginManager implements BlockManagerInterface {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Block', $namespaces, $module_handler, BlockPluginInterface::class, Block::class, 'Drupal\\Core\\Block\\Annotation\\Block');\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Attribute/FieldType.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Attribute;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass FieldType extends Plugin {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/Attribute/FieldWidget.php",
        "<?php\nnamespace Drupal\\Core\\Field\\Attribute;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\n#[\\Attribute(\\Attribute::TARGET_CLASS)]\nclass FieldWidget extends Plugin {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldTypePluginManager.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass FieldTypePluginManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Field/FieldType', $namespaces, $module_handler, FieldItemInterface::class, FieldType::class, 'Drupal\\Core\\Field\\Annotation\\FieldType');\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/WidgetPluginManager.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nuse Drupal\\Core\\Field\\Attribute\\FieldWidget;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass WidgetPluginManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Field/FieldWidget', $namespaces, $module_handler, WidgetInterface::class, FieldWidget::class, 'Drupal\\Core\\Field\\Annotation\\FieldWidget');\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Validation/ConstraintManager.php",
        "<?php\nnamespace Drupal\\Core\\Validation;\nuse Drupal\\Component\\Plugin\\Discovery\\StaticDiscoveryDecorator;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nuse Symfony\\Component\\Validator\\Constraints\\NotBlank;\nclass ConstraintManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Validation/Constraint', $namespaces, $module_handler, NULL, 'Drupal\\Core\\Validation\\Annotation\\Constraint');\n    }\n    protected function getDiscovery() {\n        if (!isset($this->discovery)) {\n            $this->discovery = parent::getDiscovery();\n            $this->discovery = new StaticDiscoveryDecorator($this->discovery, [$this, 'registerDefinitions']);\n        }\n        return $this->discovery;\n    }\n    public function registerDefinitions() {\n        $this->getDiscovery()->setDefinition('NotBlank', [\n            'class' => NotBlank::class,\n        ]);\n    }\n}\n",
    ),
    (
        "web/core/lib/Drupal/Core/TypedData/DataDefinitionInterface.php",
        "<?php\nnamespace Drupal\\Core\\TypedData;\ninterface DataDefinitionInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/FieldDefinitionInterface.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nuse Drupal\\Core\\TypedData\\DataDefinitionInterface;\ninterface FieldDefinitionInterface extends DataDefinitionInterface {}\n",
    ),
    (
        "web/core/lib/Drupal/Core/Field/BaseFieldDefinition.php",
        "<?php\nnamespace Drupal\\Core\\Field;\nclass BaseFieldDefinition implements FieldDefinitionInterface {\n    public static function create($type): static { return new static(); }\n    public function setDisplayOptions($display_context, array $options): static { return $this; }\n    public function addConstraint($constraint_name, $options = NULL): static { return $this; }\n}\n",
    ),
    (STRING_ITEM_PATH, STRING_ITEM),
    (
        WIDGET_PATH,
        "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldWidget;\nuse Drupal\\Core\\Field\\Attribute\\FieldWidget;\n#[FieldWidget(id: 'string_textfield')]\nclass StringTextfieldWidget {}\n",
    ),
    (
        CONSTRAINT_PATH,
        "<?php\nnamespace Drupal\\Core\\Validation\\Plugin\\Validation\\Constraint;\n\n/**\n * Checks that a field is unique.\n *\n * @Constraint(\n *   id = \"UniqueField\",\n *   label = @Translation(\"Unique field\")\n * )\n */\nclass UniqueFieldConstraint {}\n",
    ),
];

const STRING_ITEM_PATH: &str =
    "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldType/StringItem.php";
const STRING_ITEM: &str = "<?php\nnamespace Drupal\\Core\\Field\\Plugin\\Field\\FieldType;\nuse Drupal\\Core\\Field\\Attribute\\FieldType;\n#[FieldType(id: \"string\")]\nclass StringItem {}\n";
const WIDGET_PATH: &str =
    "web/core/lib/Drupal/Core/Field/Plugin/Field/FieldWidget/StringTextfieldWidget.php";
const CONSTRAINT_PATH: &str =
    "web/core/lib/Drupal/Core/Validation/Plugin/Validation/Constraint/UniqueFieldConstraint.php";

const HELLO_PATH: &str = "web/modules/custom/acme/src/Plugin/Block/HelloBlock.php";
const HELLO: &str = "<?php\nnamespace Drupal\\acme\\Plugin\\Block;\n\nuse Drupal\\Core\\Block\\Attribute\\Block;\n\n#[Block(id: 'acme_hello', admin_label: 'Hello')]\nfinal class HelloBlock {\n    public function greeting(): string { return ''; }\n}\n";

const LEGACY_PATH: &str = "web/modules/custom/acme/src/Plugin/Block/LegacyBlock.php";
const LEGACY: &str = "<?php\nnamespace Drupal\\acme\\Plugin\\Block;\n\n/**\n * A block, declared the way older code does.\n *\n * @Block(\n *   id = \"acme_legacy\",\n *   admin_label = @Translation(\"Legacy\")\n * )\n */\nfinal class LegacyBlock {}\n";

/// A block attribute outside the plugin subdirectory, which discovery
/// never reads.
const STRAY_PATH: &str = "web/modules/custom/acme/src/Other/StrayBlock.php";
const STRAY: &str = "<?php\nnamespace Drupal\\acme\\Other;\n\nuse Drupal\\Core\\Block\\Attribute\\Block;\n\n#[Block('acme_stray')]\nfinal class StrayBlock {}\n";

const CONSUMER_PATH: &str = "web/modules/custom/acme/src/Consumer.php";
const CONSUMER: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Block\BlockManagerInterface;
use Drupal\Core\Field\BaseFieldDefinition;

final class Consumer {
    public function __construct(private BlockManagerInterface $blocks) {}

    public function build(): void {
        $block = $this->blocks->createInstance('acme_hello');
        $block->greeting();
        $this->blocks->hasDefinition('acme_legacy');
        $this->blocks->createInstance('acme_hello:main');
        $this->blocks->getDefinition('');
        $field = BaseFieldDefinition::create('string')
            ->setDisplayOptions('form', ['type' => 'string_textfield'])
            ->addConstraint('UniqueField');
        BaseFieldDefinition::create('');
        $field->setDisplayOptions('form', ['type' => '']);
    }
}
"#;

async fn site(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    site_with(&[], files).await
}

/// [`site`], with `disk` written beside it without being opened.
async fn site_with(disk: &[(&str, &str)], files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for (path, content) in CORE
        .iter()
        .chain(&[
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".gitignore", "/web/core/\n"),
            (HELLO_PATH, HELLO),
            (LEGACY_PATH, LEGACY),
            (STRAY_PATH, STRAY),
        ])
        .chain(disk)
    {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
    }
    let (backend, dir) = workspace_in(dir, files).await;
    backend.ensure_workspace_indexed();
    (backend, dir)
}

async fn labels_at(backend: &Backend, uri: &Uri, needle: &str, inside: usize) -> Vec<String> {
    let position = position_in(CONSUMER, needle, inside);
    response_labels(
        completion_response(backend, uri, position.line, position.character, None).await,
    )
}

#[tokio::test]
async fn plugin_ids_complete_from_the_receivers_manager() {
    let (backend, dir) = site(&[(CONSUMER_PATH, CONSUMER)]).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    assert_eq!(
        labels_at(&backend, &php, "getDefinition('')", 15).await,
        ["acme_hello", "acme_legacy"]
    );
    assert_eq!(
        labels_at(&backend, &php, "BaseFieldDefinition::create('')", 29).await,
        ["string"]
    );
    assert_eq!(
        labels_at(&backend, &php, "['type' => '']", 12).await,
        ["string_textfield"]
    );
}

#[tokio::test]
async fn a_plugin_id_goes_to_its_class() {
    let (backend, dir) = site(&[(CONSUMER_PATH, CONSUMER)]).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    for (needle, target, line) in [
        ("'acme_hello')", HELLO_PATH, 6),
        ("'acme_legacy'", LEGACY_PATH, 11),
        ("'acme_hello:main'", HELLO_PATH, 6),
        ("'string')", STRING_ITEM_PATH, 4),
        ("'string_textfield'", WIDGET_PATH, 4),
        ("'UniqueField'", CONSTRAINT_PATH, 11),
    ] {
        let found = definitions_at(&backend, &php, position_in(CONSUMER, needle, 2)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&uri_for(&dir, target), line),
            "{needle}"
        );
    }
}

#[tokio::test]
async fn create_instance_is_typed_as_the_plugin_class() {
    let (backend, dir) = site(&[(CONSUMER_PATH, CONSUMER)]).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let found = definitions_at(&backend, &php, position_in(CONSUMER, "greeting()", 2)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        (&found[0].uri, found[0].range.start.line),
        (&uri_for(&dir, HELLO_PATH), 7)
    );
}

#[tokio::test]
async fn a_plugin_id_hovers_its_manager_and_class() {
    let (backend, dir) = site(&[(CONSUMER_PATH, CONSUMER)]).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let position = position_in(CONSUMER, "'acme_hello')", 2);
    let hover = crate::common::hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Plugin"), "{hover}");
    assert!(
        hover.contains("Drupal\\Core\\Block\\BlockManager"),
        "{hover}"
    );
    assert!(
        hover.contains("Drupal\\acme\\Plugin\\Block\\HelloBlock"),
        "{hover}"
    );
}

#[tokio::test]
async fn a_manager_interface_lists_the_plugins_as_implementations() {
    let (backend, dir) = site(&[(CONSUMER_PATH, CONSUMER)]).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let response = backend
        .goto_implementation(ImplementationParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: php },
                position: position_in(CONSUMER, "BlockManagerInterface $blocks", 3),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let locations = match response {
        None => Vec::new(),
        Some(ImplementationResponse::Definition(Definition::Location(location))) => vec![location],
        Some(ImplementationResponse::Definition(Definition::LocationList(locations))) => locations,
        Some(ImplementationResponse::DefinitionLinkList(_)) => panic!("unexpected location links"),
    };
    let uris: Vec<Uri> = locations.into_iter().map(|location| location.uri).collect();
    for path in [HELLO_PATH, LEGACY_PATH] {
        assert!(uris.contains(&uri_for(&dir, path)), "{path}: {uris:?}");
    }
    assert!(!uris.contains(&uri_for(&dir, STRAY_PATH)), "{uris:?}");
}

/// The implementation lens on a manager interface counts and lists what go
/// to implementation does: the manager and its plugins.
#[tokio::test]
async fn a_manager_interfaces_lens_counts_the_plugins() {
    const INTERFACE_PATH: &str = "web/core/lib/Drupal/Core/Block/BlockManagerInterface.php";
    let (backend, dir) = site(&[(CONSUMER_PATH, CONSUMER)]).await;
    let uri = uri_for(&dir, INTERFACE_PATH);
    let content = std::fs::read_to_string(dir.path().join(INTERFACE_PATH)).unwrap();
    let lens = backend
        .handle_code_lens(uri.as_str(), &content)
        .unwrap_or_default()
        .into_iter()
        .filter(|lens| lens.range.start.line == 3)
        .map(|lens| backend.resolve_code_lens_item(lens))
        .find_map(|lens| {
            lens.command
                .filter(|command| command.title.contains("implementation"))
        })
        .expect("an implementation lens");
    let locations: Vec<Location> = lens
        .arguments
        .as_ref()
        .and_then(|arguments| arguments.get(2))
        .and_then(|locations| serde_json::from_value(locations.clone()).ok())
        .unwrap_or_default();
    let uris: Vec<Uri> = locations.into_iter().map(|location| location.uri).collect();
    for path in [HELLO_PATH, LEGACY_PATH] {
        assert!(uris.contains(&uri_for(&dir, path)), "{path}: {uris:?}");
    }
    assert_eq!(
        lens.title,
        format!("{} implementations", uris.len()),
        "{uris:?}"
    );
}

const BLOCK_ANNOTATION_PATH: &str = "web/core/lib/Drupal/Core/Block/Annotation/Block.php";
const TRANSLATION_PATH: &str = "web/core/lib/Drupal/Core/Annotation/Translation.php";
/// A class named like an annotation in a plugin's own namespace, which
/// annotated discovery never reads an annotation from.
const DECOY_PATH: &str = "web/modules/custom/acme/src/Plugin/Block/Translation.php";
const IMPORTED_PATH: &str = "web/modules/custom/acme/src/Plugin/Block/ImportedBlock.php";
const IMPORTED: &str = "<?php\nnamespace Drupal\\acme\\Plugin\\Block;\n\nuse Drupal\\acme\\Plugin\\Block\\Translation;\n\n/**\n * @Block(\n *   id = \"acme_imported\",\n *   admin_label = @Translation(\"Imported\")\n * )\n */\nfinal class ImportedBlock {}\n";

/// The annotation classes, written beside the site.
const ANNOTATIONS: &[(&str, &str)] = &[
    (
        BLOCK_ANNOTATION_PATH,
        "<?php\nnamespace Drupal\\Core\\Block\\Annotation;\n\n/** @Annotation */\nclass Block {}\n",
    ),
    (
        TRANSLATION_PATH,
        "<?php\nnamespace Drupal\\Core\\Annotation;\n\n/** @Annotation */\nclass Translation {}\n",
    ),
    (
        DECOY_PATH,
        "<?php\nnamespace Drupal\\acme\\Plugin\\Block;\n\nclass Translation {}\n",
    ),
];

#[tokio::test]
async fn an_unimported_annotation_goes_where_plugin_discovery_finds_it() {
    let (backend, dir) = site_with(ANNOTATIONS, &[(LEGACY_PATH, LEGACY)]).await;
    let php = uri_for(&dir, LEGACY_PATH);
    for (needle, target) in [
        ("@Block(", BLOCK_ANNOTATION_PATH),
        ("@Translation(", TRANSLATION_PATH),
    ] {
        let found = definitions_at(&backend, &php, position_in(LEGACY, needle, 2)).await;
        assert_eq!(found.len(), 1, "{needle}: {found:?}");
        assert_eq!(
            (&found[0].uri, found[0].range.start.line),
            (&uri_for(&dir, target), 4),
            "{needle}"
        );
    }

    let position = position_in(LEGACY, "@Translation(", 2);
    let hover = crate::common::hover_text_at(&backend, &php, position.line, position.character)
        .await
        .unwrap_or_default();
    assert!(hover.contains("Drupal\\Core\\Annotation"), "{hover}");

    let found = references_at(&backend, &php, position).await;
    assert!(found.contains(&(php.clone(), 8)), "{found:?}");
    assert!(
        found.contains(&(uri_for(&dir, TRANSLATION_PATH), 4)),
        "{found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|(uri, _)| *uri == uri_for(&dir, DECOY_PATH)),
        "{found:?}"
    );
}

#[tokio::test]
async fn an_imported_annotation_goes_to_its_import() {
    let (backend, dir) = site_with(ANNOTATIONS, &[(IMPORTED_PATH, IMPORTED)]).await;
    let php = uri_for(&dir, IMPORTED_PATH);
    let found = definitions_at(&backend, &php, position_in(IMPORTED, "@Translation(", 2)).await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].uri, uri_for(&dir, DECOY_PATH));
}

fn watched(path: &std::path::Path, kind: FileChangeType) -> DidChangeWatchedFilesParams {
    DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Uri::from_file_path(path).unwrap(),
            kind,
        }],
    }
}

#[tokio::test]
async fn adding_editing_and_deleting_a_plugin_class_refreshes_the_ids() {
    let (backend, dir) = site(&[(CONSUMER_PATH, CONSUMER)]).await;
    let php = uri_for(&dir, CONSUMER_PATH);
    let offered = || labels_at(&backend, &php, "getDefinition('')", 15);
    let path = dir
        .path()
        .join("web/modules/custom/acme/src/Plugin/Block/FreshBlock.php");
    let fresh = |id: &str| {
        format!(
            "<?php\nnamespace Drupal\\acme\\Plugin\\Block;\n\nuse Drupal\\Core\\Block\\Attribute\\Block;\n\n#[Block(id: '{id}')]\nclass FreshBlock {{}}\n"
        )
    };

    std::fs::write(&path, fresh("acme_fresh")).unwrap();
    backend
        .did_change_watched_files(watched(&path, FileChangeType::Created))
        .await;
    assert!(offered().await.contains(&"acme_fresh".to_string()));

    std::fs::write(&path, fresh("acme_renamed")).unwrap();
    backend
        .did_change_watched_files(watched(&path, FileChangeType::Changed))
        .await;
    let labels = offered().await;
    assert!(labels.contains(&"acme_renamed".to_string()), "{labels:?}");
    assert!(!labels.contains(&"acme_fresh".to_string()), "{labels:?}");

    std::fs::remove_file(&path).unwrap();
    backend
        .did_change_watched_files(watched(&path, FileChangeType::Deleted))
        .await;
    assert!(!offered().await.contains(&"acme_renamed".to_string()));
}

/// A test module's block, a file that adds a block definition in code, and
/// plugin managers that find plugins their own way.
const UNKNOWN_PLUGIN_SITE: &[(&str, &str)] = &[
    (
        "web/modules/custom/acme/tests/modules/acme_test/src/Plugin/Block/TestBlock.php",
        "<?php\nnamespace Drupal\\acme_test\\Plugin\\Block;\nuse Drupal\\Core\\Block\\Attribute\\Block;\n#[Block(id: 'acme_test_block')]\nclass TestBlock {}\n",
    ),
    (
        "web/modules/custom/acme/src/RuntimeBlocks.php",
        "<?php\nnamespace Drupal\\acme;\nfinal class RuntimeBlocks {\n    public function add($manager): void {\n        $manager->setDefinition('acme_runtime', ['class' => self::class]);\n    }\n}\n",
    ),
    (
        "web/modules/custom/acme/src/OwnWayManager.php",
        "<?php\nnamespace Drupal\\acme;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass OwnWayManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/OwnWay', $namespaces, $module_handler, NULL, Plugin::class);\n    }\n    public function getDefinitions() { return []; }\n}\n",
    ),
    (
        "web/modules/custom/acme/src/YamlManager.php",
        "<?php\nnamespace Drupal\\acme;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nuse Drupal\\Core\\Plugin\\Discovery\\YamlDiscovery;\nclass YamlManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Yaml', $namespaces, $module_handler, NULL, Plugin::class);\n    }\n    protected function getDiscovery() {\n        if (!isset($this->discovery)) {\n            $this->discovery = new YamlDiscovery('acme', []);\n        }\n        return $this->discovery;\n    }\n}\n",
    ),
    (
        "web/modules/custom/acme/src/CreatorManager.php",
        "<?php\nnamespace Drupal\\acme;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass CreatorManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Creator', $namespaces, $module_handler, NULL, Plugin::class);\n    }\n    public function createInstance($plugin_id, array $configuration = []) {}\n}\n",
    ),
    (
        "web/modules/custom/acme/src/ConstantManager.php",
        "<?php\nnamespace Drupal\\acme;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\nuse Drupal\\Core\\Plugin\\DefaultPluginManager;\nclass ConstantManager extends DefaultPluginManager {\n    public function __construct(\\Traversable $namespaces, $module_handler) {\n        parent::__construct('Plugin/Constant', $namespaces, $module_handler, NULL, Plugin::class);\n    }\n}\n",
    ),
    (
        "web/modules/custom/acme/src/Plugin/Constant/Named.php",
        "<?php\nnamespace Drupal\\acme\\Plugin\\Constant;\nuse Drupal\\Component\\Plugin\\Attribute\\Plugin;\n#[Plugin(id: self::ID)]\nclass Named {\n    const ID = 'acme_named';\n}\n",
    ),
];

const LOOKUPS_PATH: &str = "web/modules/custom/acme/src/Lookups.php";
const LOOKUPS: &str = r#"<?php
namespace Drupal\acme;

use Drupal\Core\Block\BlockManagerInterface;
use Drupal\Core\Field\BaseFieldDefinition;

final class Lookups {
    public function run(BlockManagerInterface $blocks, OwnWayManager $own, YamlManager $yaml, CreatorManager $creator, ConstantManager $constant): void {
        $blocks->createInstance('acme_hello');
        $blocks->createInstance('acme_helo');
        $blocks->createInstance('acme_hello:main');
        $blocks->createInstance('acme_nothing:main');
        $blocks->createInstance('acme_test_block');
        $blocks->createInstance('acme_runtime:extra');
        $blocks->hasDefinition('acme_maybe');
        $blocks->getDefinition('acme_soft', FALSE);
        if ($blocks->hasDefinition('acme_checked')) {
            $blocks->createInstance('acme_checked');
        }
        BaseFieldDefinition::create('strin')
            ->setDisplayOptions('form', ['type' => 'string_textfeld'])
            ->addConstraint('UniqueFeld')
            ->addConstraint('NotBlank');
        $own->createInstance('acme_anything');
        $yaml->createInstance('acme_from_yaml');
        $creator->createInstance('acme_created');
        $creator->getDefinition('acme_defined');
        $constant->createInstance('acme_whatever');
    }
}
"#;

/// The unknown-plugin messages the diagnostics of `content`, open at
/// `path`, report.
fn unknown_plugins(
    backend: &Backend,
    dir: &tempfile::TempDir,
    path: &str,
    content: &str,
) -> Vec<String> {
    crate::common::slow_diagnostic_messages(
        backend,
        uri_for(dir, path).as_str(),
        content,
        "invalid_drupal_plugin",
    )
}

#[tokio::test]
async fn a_plugin_id_no_plugin_declares_is_reported() {
    let (backend, dir) = site_with(UNKNOWN_PLUGIN_SITE, &[(LOOKUPS_PATH, LOOKUPS)]).await;
    let mut found = unknown_plugins(&backend, &dir, LOOKUPS_PATH, LOOKUPS);
    found.sort();
    assert_eq!(
        found,
        [
            "Unknown plugin 'UniqueFeld' of ConstraintManager",
            "Unknown plugin 'acme_defined' of CreatorManager",
            "Unknown plugin 'acme_helo' of BlockManager",
            "Unknown plugin 'acme_nothing:main' of BlockManager",
            "Unknown plugin 'strin' of FieldTypePluginManager",
            "Unknown plugin 'string_textfeld' of WidgetPluginManager",
        ]
    );
}

#[tokio::test]
async fn a_file_that_may_catch_a_missing_plugin_checks_only_display_options() {
    let caught = LOOKUPS.replace(
        "use Drupal\\Core\\Field\\BaseFieldDefinition;",
        "use Drupal\\Core\\Field\\BaseFieldDefinition;\nuse Drupal\\Component\\Plugin\\Exception\\PluginNotFoundException;",
    );
    let (backend, dir) = site_with(UNKNOWN_PLUGIN_SITE, &[(LOOKUPS_PATH, &caught)]).await;
    assert_eq!(
        unknown_plugins(&backend, &dir, LOOKUPS_PATH, &caught),
        vec!["Unknown plugin 'string_textfeld' of WidgetPluginManager".to_string()]
    );
}

#[tokio::test]
async fn test_code_is_not_checked_for_plugins() {
    let path = "web/modules/custom/acme/tests/src/Kernel/LookupsTest.php";
    let (backend, dir) = site_with(UNKNOWN_PLUGIN_SITE, &[(path, LOOKUPS)]).await;
    assert!(unknown_plugins(&backend, &dir, path, LOOKUPS).is_empty());
}
