//! Tests for Artisan command-name and signature support.
//!
//! A command declared by a `$signature` (or `$name` / `#[AsCommand]`) is
//! surfaced when referenced as a string literal: it completes inside
//! `Artisan::call('|')`, resolves to its declaring class, and unknown names
//! are flagged.  Own arguments/options complete against the enclosing
//! command's signature.

use crate::common::{create_psr4_workspace, open_php};
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
    open_php(backend, &Url::parse(uri).unwrap(), text).await;
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
async fn signature_attribute_command_is_known() {
    let attribute_command = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Attributes\\Signature;
use Illuminate\\Console\\Command;
#[Signature('app:search:sync {--limit=50000 : Maximum queue rows to process per run}')]
class SearchSyncCommand extends Command
{
    public function handle(): void {}
}
";
    // The reference carries inline arguments; only the leading token is the
    // command name and nothing should be flagged.
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('app:search:sync --limit=50000');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("src/Console/Commands/SyncCommand.php", SYNC_COMMAND),
            (
                "src/Console/Commands/SearchSyncCommand.php",
                attribute_command,
            ),
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
    assert!(
        command_diags.is_empty(),
        "attribute-declared command should be known, got {command_diags:?}"
    );
}

#[tokio::test]
async fn inline_arguments_do_not_hide_a_bad_name() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('does:not-exist --limit=5');
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
        "the bad name should still be flagged, got {command_diags:?}"
    );
    assert!(
        command_diags[0].message.contains("'does:not-exist'"),
        "message should name only the command, got {:?}",
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

/// An apostrophe in a comment on the key's own line used to pair up with the
/// real opening quote, leaving the cursor looking like it was outside the
/// string and dropping the completion.
#[tokio::test]
async fn call_parameter_key_completes_past_an_apostrophe_in_a_comment() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('app:sync', [ /* don't ( */ '']);
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

    let position = position_after(consumer, "/* don't ( */ '");
    let labels = complete_at(&backend, &uri, position).await;
    assert!(labels.contains(&"user".to_string()), "got {labels:?}");
    assert!(labels.contains(&"--queue".to_string()), "got {labels:?}");
}

/// The parameter array may be broken over lines, so the key's opening quote is
/// not always on the same line as the call.
#[tokio::test]
async fn call_parameter_key_completes_on_a_later_line() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('app:sync', [
            '' => 1,
        ]);
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

    let position = position_after(consumer, "[\n            '");
    let labels = complete_at(&backend, &uri, position).await;
    assert!(labels.contains(&"user".to_string()), "got {labels:?}");
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

/// A package that names its command classes after the action alone and groups
/// them under `Commands/` rather than `Console/Commands/`, the way
/// `monicahq/laravel-cloudflare` ships `src/Commands/Reload.php`.
#[tokio::test]
async fn command_class_without_command_suffix_is_known() {
    let reload = "\
<?php
namespace App\\Commands;
use Illuminate\\Console\\Command;
final class Reload extends Command
{
    protected $signature = 'cloudflare:reload';
    protected $description = 'Reload trust proxies IPs and store in cache.';
    public function handle(): void {}
}
";
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('cloudflare:reload');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            // A conventionally-named command so the index is non-empty:
            // command diagnostics are skipped wholesale when nothing was
            // discovered.
            ("src/Console/Commands/SyncCommand.php", SYNC_COMMAND),
            ("src/Commands/Reload.php", reload),
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
    assert!(
        command_diags.is_empty(),
        "cloudflare:reload is declared, so it should not be flagged, got {command_diags:?}"
    );
}

/// Regression: a `#[Signature]` command registered via withCommands() from an
/// arbitrary directory (here `src/Actions/`) is known. This is the real
/// Laravel pattern `bootstrap/app.php` ->withCommands([...]) that stock
/// phpantom 0.9.0 missed — the candidate filter in build_laravel_command_index
/// only scanned `*Command`-named classes or `/Console/ /Commands/ /Command/`
/// paths, so this file was never indexed and its call was falsely flagged.
#[tokio::test]
async fn signature_attribute_command_in_actions_folder_is_known() {
    let action_command = "\
<?php
namespace App\\Actions;
use Illuminate\\Console\\Attributes\\Signature;
use Illuminate\\Console\\Command;
#[Signature('app:sync-projects')]
final class SyncProject extends Command
{
    protected $description = 'Wire the projects.';
    public function handle(): void {}
}
";
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('app:sync-projects');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            // A conventionally-named command so the index is non-empty:
            // command diagnostics are skipped wholesale when nothing was
            // discovered.
            ("src/Console/Commands/SyncCommand.php", SYNC_COMMAND),
            ("src/Actions/SyncProject.php", action_command),
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
    assert!(
        command_diags.is_empty(),
        "app:sync-projects is registered via withCommands() in src/Actions/, so it must not be flagged, got {command_diags:?}"
    );
}

/// Every name a command answers to is a valid `Artisan::call()` target: the
/// `#[Aliases]` attribute, the `protected $aliases` property, and Symfony's
/// inline `'name|alias'` form. A name none of them declares is still flagged.
#[tokio::test]
async fn command_aliases_are_not_flagged_as_unknown() {
    let aliased_commands = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Attributes\\Aliases;
use Illuminate\\Console\\Command;
use Symfony\\Component\\Console\\Attribute\\AsCommand;
#[Aliases(['app:b'])]
class BuildCommand extends Command
{
    protected $signature = 'app:build';
    public function handle(): void {}
}
class PurgeCommand extends Command
{
    protected $signature = 'app:purge';
    protected $aliases = ['app:p'];
    public function handle(): void {}
}
#[AsCommand(name: 'app:warm|app:w')]
class WarmCommand extends Command
{
    public function handle(): void {}
}
";
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Artisan;
class Runner {
    public function go(): void {
        Artisan::call('app:b');
        Artisan::call('app:p');
        Artisan::call('app:warm');
        Artisan::call('app:w');
        Artisan::call('app:nope');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("src/Console/Commands/Aliased.php", aliased_commands),
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
        "only app:nope is undeclared, got {command_diags:?}"
    );
    assert!(
        command_diags[0].message.contains("app:nope"),
        "expected the unknown name to be app:nope, got {command_diags:?}"
    );
}

// ─── Signature-typed accessors ─────────────────────────────────────────────

const TYPED_COMMAND: &str = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class TypedCommand extends Command
{
    protected $signature = 'app:typed {user} {slug?} {tags*} {--queue} {--format=} {--limit=10} {--id=*}';
    public function handle(): void
    {
        $user = $this->argument('user');
        $slug = $this->argument('slug');
        $tags = $this->argument('tags');
        $queue = $this->option('queue');
        $format = $this->option('format');
        $limit = $this->option('limit');
        $ids = $this->option('id');
        $unknown = $this->option('nope');
    }
}
";

const ATTRIBUTE_TYPED_COMMAND: &str = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Attributes\\Signature;
use Illuminate\\Console\\Command;
#[Signature('app:attributed {--days=7}')]
class AttributedCommand extends Command
{
    public function handle(): void
    {
        $days = $this->option('days');
    }
}
";

/// Hover text for the first occurrence of `needle` in `TYPED_COMMAND`.
async fn hover_in_typed_command(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    needle: &str,
) -> String {
    let position = position_after(TYPED_COMMAND, needle);
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();
    match hover.map(|h| h.contents) {
        Some(HoverContents::Markup(markup)) => markup.value,
        Some(HoverContents::Scalar(MarkedString::String(s))) => s,
        Some(HoverContents::Scalar(MarkedString::LanguageString(ls))) => ls.value,
        _ => String::new(),
    }
}

#[tokio::test]
async fn signature_types_argument_and_option_accessors() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[("src/Console/Commands/TypedCommand.php", TYPED_COMMAND)],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Console/Commands/TypedCommand.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, TYPED_COMMAND).await;

    for (needle, expected) in [
        // `{user}` is required, so it always arrives.
        ("$user", "$user = string"),
        // `{slug?}` may be left out and carries no default.
        ("$slug", "$slug = ?string"),
        // `{tags*}` collects every value it is given.
        ("$tags", "$tags = list<string>"),
        // `{--queue}` takes no value, so it is a flag.
        ("$queue", "$queue = bool"),
        // `{--format=}` takes a value but has no default.
        ("$format", "$format = ?string"),
        // `{--limit=10}` always has a value.
        ("$limit", "$limit = string"),
        // `{--id=*}` is an array option.
        ("$ids", "$ids = list<string>"),
    ] {
        let text = hover_in_typed_command(&backend, &uri, needle).await;
        assert!(
            text.contains(expected),
            "expected `{expected}` in hover for {needle}, got: {text}"
        );
    }

    // A name the signature does not declare stays on the framework's own
    // declared type rather than being invented.
    let unknown = hover_in_typed_command(&backend, &uri, "$unknown").await;
    assert!(
        !unknown.contains("bool") && !unknown.contains("string"),
        "an undeclared option must not be typed from the signature, got: {unknown}"
    );
}

#[tokio::test]
async fn an_attribute_declared_signature_types_the_accessors_too() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[(
            "src/Console/Commands/AttributedCommand.php",
            ATTRIBUTE_TYPED_COMMAND,
        )],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(
        dir.path()
            .join("src/Console/Commands/AttributedCommand.php"),
    )
    .unwrap()
    .to_string();
    open(&backend, &uri, ATTRIBUTE_TYPED_COMMAND).await;

    let position = position_after(ATTRIBUTE_TYPED_COMMAND, "$days");
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();
    let text = match hover.map(|h| h.contents) {
        Some(HoverContents::Markup(markup)) => markup.value,
        _ => String::new(),
    };
    assert!(text.contains("$days = string"), "got: {text}");
}

#[tokio::test]
async fn a_command_that_writes_its_own_accessor_keeps_its_declared_type() {
    const OVERRIDING_COMMAND: &str = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class OverridingCommand extends Command
{
    protected $signature = 'app:overriding {--flag}';
    public function option($key = null): int { return 0; }
    public function handle(): void
    {
        $flag = $this->option('flag');
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[(
            "src/Console/Commands/OverridingCommand.php",
            OVERRIDING_COMMAND,
        )],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(
        dir.path()
            .join("src/Console/Commands/OverridingCommand.php"),
    )
    .unwrap()
    .to_string();
    open(&backend, &uri, OVERRIDING_COMMAND).await;

    let position = position_after(OVERRIDING_COMMAND, "$flag");
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();
    let text = match hover.map(|h| h.contents) {
        Some(HoverContents::Markup(markup)) => markup.value,
        _ => String::new(),
    };
    assert!(
        text.contains("$flag = int"),
        "the command's own return type wins over the signature, got: {text}"
    );
}

/// The `?? []` an `option()` call is so often given leaves `string|array{}`,
/// and the truthy guard that follows proves the string half.  Reporting the
/// empty array against a `string` parameter inside that guard is a false
/// positive on the everyday shape of a console command.
#[tokio::test]
async fn a_single_value_option_survives_a_default_and_a_truthy_guard() {
    const GUARDED_COMMAND: &str = "\
<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class GuardedCommand extends Command
{
    protected $signature = 'app:guarded {--markets=}';
    public function handle(): void
    {
        $markets = $this->option('markets') ?? [];
        if ($markets) {
            $ids = explode(',', $markets);
        }
    }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[("src/Console/Commands/GuardedCommand.php", GUARDED_COMMAND)],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Console/Commands/GuardedCommand.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, GUARDED_COMMAND).await;

    let position = position_after(GUARDED_COMMAND, "        if ($markets");
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap();
    let text = match hover.map(|h| h.contents) {
        Some(HoverContents::Markup(markup)) => markup.value,
        _ => String::new(),
    };
    assert!(
        text.contains("$markets = string"),
        "the guard proves the string half of `?string ?? []`, got: {text}"
    );

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, GUARDED_COMMAND, &mut diags);
    let mismatches: Vec<&Diagnostic> = diags
        .iter()
        .filter(
            |d| matches!(&d.code, Some(NumberOrString::String(c)) if c == "type_mismatch_argument"),
        )
        .collect();
    assert!(
        mismatches.is_empty(),
        "explode() is handed a string inside the guard, got {mismatches:?}"
    );
}
