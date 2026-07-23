//! Tests for Artisan command-name and signature support.
//!
//! A command declared by a `$signature` (or `$name` / `#[AsCommand]`) is
//! surfaced when referenced as a string literal: it completes inside
//! `Artisan::call('|')`, resolves to its declaring class, and unknown names
//! are flagged.  Own arguments/options complete against the enclosing
//! command's signature.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const SYNC_COMMAND: &str = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class SyncCommand extends Command
{
    protected $signature = 'app:sync {user} {--queue}';
    protected $description = 'Sync the things';
    public function handle(): void {}
}
";

const REPORT_COMMAND: &str = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class ReportCommand extends Command
{
    protected $signature = 'reports:build {--format=}';
    public function handle(): void {}
}
";

async fn open(backend: &phpantom_lsp::Backend, uri: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri).unwrap(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

/// Position of the cursor immediately after the first occurrence of `needle`.
fn position_after(content: &str, needle: &str) -> Position {
    let idx = content.find(needle).expect("needle not found") + needle.len();
    let mut line = 0u32;
    let mut character = 0u32;
    for (i, ch) in content.char_indices() {
        if i == idx {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }
    Position { line, character }
}

fn completion_labels(response: Option<CompletionResponse>) -> Vec<String> {
    match response {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        Some(CompletionResponse::List(list)) => list.items.into_iter().map(|i| i.label).collect(),
        None => Vec::new(),
    }
}

async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    position: Position,
) -> Vec<String> {
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    completion_labels(result)
}

#[tokio::test]
async fn command_name_completes_in_artisan_call() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("src/Console/Commands/SyncCommand.php", SYNC_COMMAND),
            ("src/Console/Commands/ReportCommand.php", REPORT_COMMAND),
            ("src/Runner.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Runner.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;

    let position = position_after(consumer, "Artisan::call('");
    let labels = complete_at(&backend, &uri, position).await;
    assert!(
        labels.contains(&"app:sync".to_string()),
        "expected app:sync in {labels:?}"
    );
    assert!(
        labels.contains(&"reports:build".to_string()),
        "expected reports:build in {labels:?}"
    );
}

#[tokio::test]
async fn command_name_resolves_to_declaring_class() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('app:sync');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("src/Console/Commands/SyncCommand.php", SYNC_COMMAND),
            ("src/Runner.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Runner.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;

    // Cursor on `app:sync` inside the string.
    let position = position_after(consumer, "Artisan::call('app");
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();

    let target = match result.expect("app:sync should resolve") {
        GotoDefinitionResponse::Scalar(loc) => loc.uri,
        GotoDefinitionResponse::Array(locs) => locs.into_iter().next().unwrap().uri,
        GotoDefinitionResponse::Link(links) => links.into_iter().next().unwrap().target_uri,
    };
    assert!(
        target
            .as_str()
            .ends_with("/Console/Commands/SyncCommand.php"),
        "should jump to SyncCommand.php, got {target}"
    );
}

#[tokio::test]
async fn unknown_command_name_is_flagged() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('app:sync');
        Artisan::call('does:not-exist');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("src/Console/Commands/SyncCommand.php", SYNC_COMMAND),
            ("src/Runner.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Runner.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);

    let command_diags: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| {
            matches!(&d.code, Some(NumberOrString::String(c)) if c == "invalid_laravel_command")
        })
        .collect();
    assert_eq!(
        command_diags.len(),
        1,
        "exactly one unknown command should be flagged, got {command_diags:?}"
    );
    assert!(
        command_diags[0].message.contains("does:not-exist"),
        "message should name the bad command, got {:?}",
        command_diags[0].message
    );
}

#[tokio::test]
async fn own_option_completes_against_signature() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[("src/Console/Commands/SyncCommand.php", SYNC_COMMAND)],
    );
    backend.initialized(InitializedParams {}).await;

    // Edit the command to reference its own option inside handle().
    let edited = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class SyncCommand extends Command
{
    protected $signature = 'app:sync {user} {--queue} {--conn=}';
    public function handle(): void {
        $this->option('');
    }
}
";
    let uri = Url::from_file_path(dir.path().join("src/Console/Commands/SyncCommand.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, edited).await;

    let position = position_after(edited, "$this->option('");
    let labels = complete_at(&backend, &uri, position).await;
    assert!(labels.contains(&"queue".to_string()), "got {labels:?}");
    assert!(labels.contains(&"conn".to_string()), "got {labels:?}");
    assert!(
        !labels.contains(&"user".to_string()),
        "arguments should not appear for option(), got {labels:?}"
    );
}

#[tokio::test]
async fn unknown_own_argument_is_flagged() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[("src/Console/Commands/SyncCommand.php", SYNC_COMMAND)],
    );
    backend.initialized(InitializedParams {}).await;

    let edited = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class SyncCommand extends Command
{
    protected $signature = 'app:sync {user}';
    public function handle(): void {
        $this->argument('user');
        $this->argument('nope');
    }
}
";
    let uri = Url::from_file_path(dir.path().join("src/Console/Commands/SyncCommand.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, edited).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, edited, &mut diags);

    let param_diags: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| {
            matches!(&d.code, Some(NumberOrString::String(c)) if c == "invalid_command_parameter")
        })
        .collect();
    assert_eq!(
        param_diags.len(),
        1,
        "only the unknown argument should be flagged, got {param_diags:?}"
    );
    assert!(
        param_diags[0].message.contains("nope"),
        "got {:?}",
        param_diags[0].message
    );
}
