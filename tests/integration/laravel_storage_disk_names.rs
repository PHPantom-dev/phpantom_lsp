//! End-to-end coverage for Laravel storage disk names backed by config keys.

use crate::common::{
    complete_labels_at_opened, create_psr4_workspace, open_php_at, position_after,
};
use phpantom_lsp::Backend;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^12.0" },
    "autoload": { "psr-4": { "App\\": "app/" } }
}"#;

const FILESYSTEMS_CONFIG: &str = r#"<?php
return [
    'default' => 'local',
    'disks' => [
        'local' => ['driver' => 'local'],
        'archive' => ['driver' => 'local'],
        'backup' => ['driver' => 's3'],
    ],
];
"#;

async fn open_workspace(source: &str) -> (Backend, tempfile::TempDir, Url) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("config/filesystems.php", FILESYSTEMS_CONFIG),
            ("app/DiskConsumer.php", source),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = open_php_at(&backend, &dir, "app/DiskConsumer.php", source).await;

    (backend, dir, uri)
}

async fn completion_labels(backend: &Backend, uri: &Url, position: Position) -> Vec<String> {
    complete_labels_at_opened(backend, uri, position.line, position.character).await
}

fn definition_location(response: GotoDefinitionResponse) -> Location {
    match response {
        GotoDefinitionResponse::Scalar(location) => location,
        GotoDefinitionResponse::Array(mut locations) => locations.remove(0),
        GotoDefinitionResponse::Link(mut links) => {
            let link = links.remove(0);
            Location::new(link.target_uri, link.target_selection_range)
        }
    }
}

fn sorted_locations(locations: Vec<Location>) -> Vec<String> {
    let mut locations = locations
        .into_iter()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.uri, location.range.start.line, location.range.start.character
            )
        })
        .collect::<Vec<_>>();
    locations.sort_unstable();
    locations
}

#[tokio::test]
async fn scalar_array_and_named_storage_arguments_complete_configured_disks() {
    let source = r#"<?php
use Illuminate\Support\Facades\Storage;

Storage::disk('');
Storage::disk(name: 'a');
Storage::fake(disk: 'a');
Storage::persistentFake(config: [], disk: 'ar');
Storage::forgetDisk('l');
Storage::forgetDisk(disk: ['local', 'b']);
Storage::forgetDisk(disk: array('ar'));
#[\Illuminate\Container\Attributes\Storage(disk: 'a')]
class DiskConsumer {}
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    let all = completion_labels(&backend, &uri, position_after(source, "Storage::disk('")).await;
    assert_eq!(all, ["archive", "backup", "local"]);

    for prefix in [
        "Storage::disk(name: 'a",
        "Storage::fake(disk: 'a",
        "Storage::persistentFake(config: [], disk: 'ar",
        "Attributes\\Storage(disk: 'a",
    ] {
        let labels = completion_labels(&backend, &uri, position_after(source, prefix)).await;
        assert_eq!(labels, ["archive"], "completion at `{prefix}`");
    }

    let local = completion_labels(
        &backend,
        &uri,
        position_after(source, "Storage::forgetDisk('l"),
    )
    .await;
    assert_eq!(local, ["local"]);

    let array_value = completion_labels(
        &backend,
        &uri,
        position_after(source, "Storage::forgetDisk(disk: ['local', 'b"),
    )
    .await;
    assert_eq!(array_value, ["backup"]);

    let legacy_array_value = completion_labels(
        &backend,
        &uri,
        position_after(source, "Storage::forgetDisk(disk: array('ar"),
    )
    .await;
    assert_eq!(legacy_array_value, ["archive"]);
}

#[tokio::test]
async fn storage_completion_rejects_array_keys_wrong_parameters_and_wrong_shapes() {
    let source = r#"<?php
use Illuminate\Support\Facades\Storage;

Storage::disk(['a']);
Storage::fake(['a']);
Storage::persistentFake(['a']);
Storage::forgetDisk(config: 'a');
Storage::forgetDisk(['ar' => true]);
Storage::forgetDisk(array('ba' => true));
#[\Illuminate\Container\Attributes\Storage(disk: ['a'])]
class DiskConsumer {}
"#;
    let (backend, _dir, uri) = open_workspace(source).await;

    for prefix in [
        "Storage::disk(['a",
        "Storage::fake(['a",
        "Storage::persistentFake(['a",
        "Storage::forgetDisk(config: 'a",
        "Storage::forgetDisk(['ar",
        "Storage::forgetDisk(array('ba",
        "Attributes\\Storage(disk: ['a",
    ] {
        let labels = completion_labels(&backend, &uri, position_after(source, prefix)).await;
        assert!(
            labels
                .iter()
                .all(|label| !["archive", "backup", "local"].contains(&label.as_str())),
            "invalid call shape at `{prefix}` offered storage disks: {labels:?}"
        );
    }
}

#[tokio::test]
async fn every_storage_context_navigates_and_hovers_as_a_storage_disk() {
    let source = r#"<?php
use Illuminate\Support\Facades\Storage;

Storage::disk('archive');
Storage::fake('archive');
Storage::persistentFake('archive');
Storage::forgetDisk('archive');
Storage::forgetDisk(['archive', 'backup']);
Storage::forgetDisk(array('archive'));
#[\Illuminate\Container\Attributes\Storage('archive')]
class DiskConsumer {}
"#;
    let (backend, _dir, uri) = open_workspace(source).await;
    let positions = [
        position_after(source, "Storage::disk('arch"),
        position_after(source, "Storage::fake('arch"),
        position_after(source, "Storage::persistentFake('arch"),
        position_after(source, "Storage::forgetDisk('arch"),
        position_after(source, "Storage::forgetDisk(['arch"),
        position_after(source, "Storage::forgetDisk(array('arch"),
        position_after(source, "Attributes\\Storage('arch"),
    ];

    for position in positions {
        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("definition request should succeed")
            .expect("configured disk should have a definition");
        let location = definition_location(response);
        assert!(location.uri.path().ends_with("/config/filesystems.php"));
        assert_eq!(location.range.start.line, 5, "archive config key line");

        let hover = backend
            .handle_hover(uri.as_str(), source, position)
            .expect("configured disk should have hover");
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("expected markdown hover");
        };
        assert!(markup.value.contains("**Storage disk** `archive`"));
        assert!(markup.value.contains("config/filesystems.php"));
    }
}

#[tokio::test]
async fn group_imported_storage_aliases_share_intelligence_without_claiming_local_homonyms() {
    let source = r#"<?php
namespace App;

use Illuminate\Support\Facades\{Cache, Storage as Disks};
use Illuminate\Container\Attributes\{Config, Storage as Disk};

#[\Attribute(\Attribute::TARGET_PARAMETER)]
class Storage {
    public function __construct(string $disk) {}
    public static function disk(string $name): void {}
}

class DiskConsumer {
    public function __construct(
        #[Disk(disk: 'archive')] mixed $configured,
        #[Storage(disk: 'archive')] mixed $local,
        #[Storage(disk: 'missing-local-attribute')] mixed $unconfigured,
    ) {}

    public function run(): void {
        Disks::disk(name: 'archive');
        config('filesystems.disks.archive');
        Storage::disk('archive');
        Storage::disk('missing-local-disk');
    }
}
"#;
    let (backend, dir, uri) = open_workspace(source).await;
    let config_uri = Url::from_file_path(dir.path().join("config/filesystems.php")).unwrap();
    let declaration_position = position_after(FILESYSTEMS_CONFIG, "'arch");
    let expected_references = backend
        .find_references(
            config_uri.as_str(),
            FILESYSTEMS_CONFIG,
            declaration_position,
            true,
        )
        .expect("configured disk declaration should have references");
    assert_eq!(
        expected_references.len(),
        4,
        "aliased facade, aliased attribute, config read and declaration: {expected_references:#?}"
    );
    let expected_references = sorted_locations(expected_references);

    for prefix in ["Disks::disk(name: 'arch", "#[Disk(disk: 'arch"] {
        let position = position_after(source, prefix);
        assert_eq!(
            completion_labels(&backend, &uri, position).await,
            ["archive"],
            "group-imported alias at `{prefix}` should complete configured disks"
        );

        let response = backend
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("definition request should succeed")
            .unwrap_or_else(|| panic!("group-imported alias at `{prefix}` should navigate"));
        let location = definition_location(response);
        assert_eq!(location.uri, config_uri);
        assert_eq!(location.range.start.line, declaration_position.line);

        let hover = backend
            .handle_hover(uri.as_str(), source, position)
            .unwrap_or_else(|| panic!("group-imported alias at `{prefix}` should hover"));
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("expected markdown hover");
        };
        assert!(markup.value.contains("**Storage disk** `archive`"));
        assert!(markup.value.contains("config/filesystems.php"));

        let references = backend
            .find_references(uri.as_str(), source, position, true)
            .unwrap_or_else(|| panic!("group-imported alias at `{prefix}` should have references"));
        assert_eq!(sorted_locations(references), expected_references);
    }
    let config_references = backend
        .find_references(
            uri.as_str(),
            source,
            position_after(source, "config('filesystems.disks.arch"),
            true,
        )
        .expect("generic config access should share the aliased disk references");
    assert_eq!(sorted_locations(config_references), expected_references);

    for prefix in ["Storage::disk('arch", "#[Storage(disk: 'arch"] {
        let position = position_after(source, prefix);
        let labels = completion_labels(&backend, &uri, position).await;
        assert!(
            labels
                .iter()
                .all(|label| !["archive", "backup", "local"].contains(&label.as_str())),
            "local homonym at `{prefix}` offered storage disks: {labels:?}"
        );
        assert!(
            backend
                .handle_hover(uri.as_str(), source, position)
                .is_none(),
            "local homonym at `{prefix}` should not have disk hover"
        );
        assert!(
            backend
                .find_references(uri.as_str(), source, position, true)
                .is_none(),
            "local homonym at `{prefix}` should not have disk references"
        );
        assert!(
            backend
                .goto_definition(GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri: uri.clone() },
                        position,
                    },
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                })
                .await
                .expect("definition request should succeed")
                .is_none(),
            "local homonym at `{prefix}` should not navigate to disk configuration"
        );
    }

    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    assert!(
        diagnostics.iter().all(|diagnostic| {
            !matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code == "invalid_laravel_storage_disk"
            )
        }),
        "configured aliases are valid and local homonyms name no disk: {diagnostics:#?}"
    );
}

#[tokio::test]
async fn only_storage_contexts_that_require_a_configured_disk_are_diagnosed() {
    let source = r#"<?php
use Illuminate\Support\Facades\Storage;

Storage::disk('missing-disk');
Storage::fake(config: [], disk: 'testing');
Storage::persistentFake(disk: 'persistent-testing');
Storage::forgetDisk('already-forgotten');
Storage::forgetDisk(['forgotten-one', 'forgotten-two']);
Storage::forgetDisk(array('forgotten-three'));
#[\Illuminate\Container\Attributes\Storage('missing-attribute')]
class DiskConsumer {}
"#;
    let (backend, _dir, uri) = open_workspace(source).await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);

    let invalid_disks = diagnostics
        .iter()
        .filter(|diagnostic| {
            matches!(
                &diagnostic.code,
                Some(NumberOrString::String(code)) if code == "invalid_laravel_storage_disk"
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(invalid_disks.len(), 2, "got: {invalid_disks:#?}");

    let messages = invalid_disks
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("storage disk: 'missing-disk'"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("storage disk: 'missing-attribute'"))
    );
    for optional_or_written in [
        "testing",
        "persistent-testing",
        "already-forgotten",
        "forgotten-one",
        "forgotten-two",
        "forgotten-three",
    ] {
        assert!(
            messages
                .iter()
                .all(|message| !message.contains(optional_or_written)),
            "`{optional_or_written}` should not be diagnosed: {messages:?}"
        );
    }
}

#[tokio::test]
async fn storage_calls_generic_config_access_and_declaration_share_references_symmetrically() {
    let source = r#"<?php
use Illuminate\Support\Facades\Storage;

Storage::disk('archive');
Storage::fake('archive');
Storage::persistentFake('archive');
Storage::forgetDisk('archive');
Storage::forgetDisk(['archive', 'backup']);
Storage::forgetDisk(array('archive'));
config('filesystems.disks.archive');
#[\Illuminate\Container\Attributes\Storage('archive')]
class DiskConsumer {}
"#;
    let (backend, dir, uri) = open_workspace(source).await;

    let storage_references = backend
        .find_references(
            uri.as_str(),
            source,
            position_after(source, "Storage::fake('arch"),
            true,
        )
        .expect("storage disk should have references");
    let config_references = backend
        .find_references(
            uri.as_str(),
            source,
            position_after(source, "config('filesystems.disks.arch"),
            true,
        )
        .expect("generic config key should have references");

    let config_uri = Url::from_file_path(dir.path().join("config/filesystems.php")).unwrap();
    let declaration_references = backend
        .find_references(
            config_uri.as_str(),
            FILESYSTEMS_CONFIG,
            position_after(FILESYSTEMS_CONFIG, "'arch"),
            true,
        )
        .expect("config declaration should have references");

    assert_eq!(
        storage_references.len(),
        9,
        "seven call usages, one attribute usage, and one declaration: {storage_references:#?}"
    );
    let expected = sorted_locations(storage_references);
    assert_eq!(sorted_locations(config_references), expected);
    assert_eq!(sorted_locations(declaration_references), expected);
}
