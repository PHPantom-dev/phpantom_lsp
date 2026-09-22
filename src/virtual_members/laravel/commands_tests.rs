use super::*;

fn arg_names(sig: &CommandSignature) -> Vec<&str> {
    sig.arguments.iter().map(|p| p.name.as_str()).collect()
}

fn opt_names(sig: &CommandSignature) -> Vec<&str> {
    sig.options.iter().map(|p| p.name.as_str()).collect()
}

#[test]
fn parses_command_name() {
    let sig = parse_signature("app:sync {user}");
    assert_eq!(sig.name, "app:sync");
}

#[test]
fn parses_name_only_signature() {
    let sig = parse_signature("mail:send");
    assert_eq!(sig.name, "mail:send");
    assert!(sig.arguments.is_empty());
    assert!(sig.options.is_empty());
}

#[test]
fn parses_required_and_optional_arguments() {
    let sig = parse_signature("app:sync {user} {team?}");
    assert_eq!(arg_names(&sig), vec!["user", "team"]);
    assert!(!sig.argument("user").unwrap().optional);
    assert!(sig.argument("team").unwrap().optional);
}

#[test]
fn parses_array_arguments() {
    let sig = parse_signature("app:sync {user*} {team?*}");
    assert!(sig.argument("user").unwrap().is_array);
    assert!(!sig.argument("user").unwrap().optional);
    assert!(sig.argument("team").unwrap().is_array);
    assert!(sig.argument("team").unwrap().optional);
}

#[test]
fn parses_argument_default() {
    let sig = parse_signature("app:sync {user=guest}");
    let user = sig.argument("user").unwrap();
    assert_eq!(user.default.as_deref(), Some("guest"));
    assert!(user.optional);
}

#[test]
fn parses_options() {
    let sig = parse_signature("app:sync {--queue} {--connection=}");
    assert_eq!(opt_names(&sig), vec!["queue", "connection"]);
    // Flag: no value.
    assert!(!sig.option("queue").unwrap().takes_value);
    // Value option.
    assert!(sig.option("connection").unwrap().takes_value);
}

#[test]
fn parses_option_default_and_shortcut() {
    let sig = parse_signature("app:sync {--Q|queue=default}");
    let queue = sig.option("queue").unwrap();
    assert_eq!(queue.shortcut.as_deref(), Some("Q"));
    assert_eq!(queue.default.as_deref(), Some("default"));
    assert!(queue.takes_value);
}

#[test]
fn parses_array_option() {
    let sig = parse_signature("app:sync {--id=*}");
    let id = sig.option("id").unwrap();
    assert!(id.is_array);
    assert!(id.takes_value);
}

#[test]
fn parses_descriptions() {
    let sig = parse_signature("app:sync {user : The user ID} {--queue : Queue the job}");
    assert_eq!(
        sig.argument("user").unwrap().description.as_deref(),
        Some("The user ID")
    );
    assert_eq!(
        sig.option("queue").unwrap().description.as_deref(),
        Some("Queue the job")
    );
}

#[test]
fn parses_multiline_signature() {
    let sig = parse_signature(
        "app:sync
            {user : The user}
            {--queue : Whether to queue}",
    );
    assert_eq!(sig.name, "app:sync");
    assert_eq!(arg_names(&sig), vec!["user"]);
    assert_eq!(opt_names(&sig), vec!["queue"]);
}

#[test]
fn scans_signature_command_class() {
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Command;

class SyncCommand extends Command
{
    protected $signature = 'app:sync {user} {--queue}';
    protected $description = 'Sync stuff';
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/SyncCommand.php");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.name, "app:sync");
    assert_eq!(
        entry.fqn.as_deref(),
        Some("App\\Console\\Commands\\SyncCommand")
    );
    assert_eq!(arg_names(&entry.signature), vec!["user"]);
    assert_eq!(opt_names(&entry.signature), vec!["queue"]);
    // Offset points inside the string literal (at `app:sync`).
    let at = &content[entry.name_offset as usize..];
    assert!(at.starts_with("app:sync"));
}

#[test]
fn scans_signature_attribute_command_class() {
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Attributes\Signature;
use Illuminate\Console\Command;

#[Signature('app:search:sync {--limit=50000 : Maximum queue rows to process per run}')]
class SearchSyncCommand extends Command
{
}
"#;
    let entries = scan_command_file(
        content,
        "file:///app/Console/Commands/SearchSyncCommand.php",
    );
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.name, "app:search:sync");
    assert_eq!(opt_names(&entry.signature), vec!["limit"]);
    let at = &content[entry.name_offset as usize..];
    assert!(at.starts_with("app:search:sync"));
}

#[test]
fn signature_attribute_wins_over_property() {
    // Laravel's configureFromAttributes() assigns the attribute over the
    // property, so the attribute is the effective signature.
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Attributes\Signature;
use Illuminate\Console\Command;

#[Signature('app:new {user}')]
class SyncCommand extends Command
{
    protected $signature = 'app:old {team}';
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/SyncCommand.php");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "app:new");
    assert_eq!(arg_names(&entries[0].signature), vec!["user"]);
}

#[test]
fn scans_name_only_command_class() {
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Command;

class LegacyCommand extends Command
{
    protected $name = 'legacy:run';
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/LegacyCommand.php");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "legacy:run");
}

#[test]
fn scans_as_command_attribute() {
    let content = r#"<?php
namespace App\Console\Commands;

use Symfony\Component\Console\Attribute\AsCommand;
use Illuminate\Console\Command;

#[AsCommand(name: 'reports:build')]
class BuildReports extends Command
{
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/BuildReports.php");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "reports:build");
    let at = &content[entries[0].name_offset as usize..];
    assert!(at.starts_with("reports:build"));
}

#[test]
fn signature_wins_over_as_command_attribute() {
    // Command::__construct() takes the signature branch whenever a signature
    // is set and never reaches Symfony's getDefaultName(), so Artisan
    // registers the signature's name.
    let content = r#"<?php
namespace App\Console\Commands;

use Symfony\Component\Console\Attribute\AsCommand;
use Illuminate\Console\Command;

#[AsCommand(name: 'x:from-as-command')]
class Sync extends Command
{
    protected $signature = 'x:from-property {user}';
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/Sync.php");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "x:from-property");
    assert_eq!(entries[0].signature.name, "x:from-property");
    assert_eq!(arg_names(&entries[0].signature), vec!["user"]);
    let at = &content[entries[0].name_offset as usize..];
    assert!(at.starts_with("x:from-property"));
}

#[test]
fn name_property_wins_over_as_command_attribute() {
    // Without a signature the constructor passes $this->name to the parent,
    // which short-circuits getDefaultName().
    let content = r#"<?php
namespace App\Console\Commands;

use Symfony\Component\Console\Attribute\AsCommand;
use Illuminate\Console\Command;

#[AsCommand(name: 'x:from-as-command')]
class Sync extends Command
{
    protected $name = 'x:from-name';
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/Sync.php");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "x:from-name");
    let at = &content[entries[0].name_offset as usize..];
    assert!(at.starts_with("x:from-name"));
}

#[test]
fn scans_command_class_without_command_suffix() {
    // monicahq/laravel-cloudflare ships src/Commands/Reload.php: the class
    // name carries no `Command` suffix and the directory is not
    // `Console/Commands`.
    let content = r#"<?php
namespace Monicahq\Cloudflare\Commands;

use Illuminate\Console\Command;

final class Reload extends Command
{
    protected $signature = 'cloudflare:reload';
    protected $description = 'Reload trust proxies IPs and store in cache.';
}
"#;
    let uri = "file:///vendor/monicahq/laravel-cloudflare/src/Commands/Reload.php";
    assert!(is_command_directory_uri(uri));
    let entries = scan_command_file(content, uri);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "cloudflare:reload");
    assert_eq!(
        entries[0].fqn.as_deref(),
        Some("Monicahq\\Cloudflare\\Commands\\Reload")
    );
}

#[test]
fn ignores_non_command_class() {
    let content = r#"<?php
namespace App\Models;

class User
{
    protected $signature = 'not a command';
}
"#;
    let entries = scan_command_file(content, "file:///app/Models/User.php");
    assert!(entries.is_empty());
}

#[test]
fn indexes_standalone_aliases_attribute() {
    let content = r#"<?php
namespace App\Actions\Sync;

use Illuminate\Console\Attributes\Aliases;
use Illuminate\Console\Attributes\Signature;
use Illuminate\Console\Command;

#[Signature('sync:projects {--force}')]
#[Aliases(['sync:p'])]
class SyncProjects extends Command
{
}
"#;
    let entries = scan_command_file(content, "file:///app/Actions/Sync/SyncProjects.php");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.name, "sync:projects");
    assert_eq!(entry.aliases, vec!["sync:p"]);
}

#[test]
fn indexes_signature_and_as_command_aliases() {
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Attributes\Signature;
use Illuminate\Console\Command;
use Symfony\Component\Console\Attribute\AsCommand;

#[Signature('mail:send', aliases: ['mail:go'])]
class MailSend extends Command
{
}

#[AsCommand(name: 'cache:clear', aliases: ['cache:clean'])]
class CacheClear extends Command
{
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/AliasesCheck.php");
    assert_eq!(entries.len(), 2);
    let by_name = |n: &str| entries.iter().find(|e| e.name == n).unwrap();
    assert_eq!(by_name("mail:send").aliases, vec!["mail:go"]);
    assert_eq!(by_name("cache:clear").aliases, vec!["cache:clean"]);
}

#[test]
fn index_resolves_alias_names() {
    let mut index = LaravelCommandIndex::default();
    index.files.set_file(
        "file:///a.php".to_string(),
        scan_command_file(
            "<?php #[Signature('a:run', aliases: ['a:go', 'a:fly'])] class ARun extends Command { }",
            "file:///a.php",
        ),
    );
    index.rebuild();

    assert!(index.get("a:run").is_some());
    assert!(index.get("a:go").is_some());
    assert!(index.get("a:fly").is_some());
    assert!(index.get("a:nope").is_none());
    assert_eq!(index.all_names(), vec!["a:fly", "a:go", "a:run"]);
}

#[test]
fn indexes_aliases_property() {
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Command;

class SyncCommand extends Command
{
    protected $signature = 'app:sync';
    protected $aliases = ['app:s', 'sync'];
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/SyncCommand.php");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "app:sync");
    assert_eq!(entries[0].aliases, vec!["app:s", "sync"]);
}

/// `configureFromAttributes()` assigns over the property, so an attribute
/// alias list replaces `$aliases` rather than merging with it.
#[test]
fn attribute_aliases_override_the_aliases_property() {
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Attributes\Aliases;
use Illuminate\Console\Command;

#[Aliases(['from:attribute'])]
class SyncCommand extends Command
{
    protected $signature = 'app:sync';
    protected $aliases = ['from:property'];
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/SyncCommand.php");
    assert_eq!(entries[0].aliases, vec!["from:attribute"]);
}

/// Symfony splits `name|alias1|alias2` in `Command::__construct()`, so the
/// primary name must not be indexed with the pipes still in it.
#[test]
fn splits_symfony_piped_command_names() {
    let content = r#"<?php
namespace App\Console\Commands;

use Illuminate\Console\Command;
use Symfony\Component\Console\Attribute\AsCommand;

#[AsCommand(name: 'app:sync|app:s|sync')]
class SyncCommand extends Command
{
}

class PurgeCommand extends Command
{
    protected $name = 'app:purge|app:p';
}
"#;
    let entries = scan_command_file(content, "file:///app/Console/Commands/Piped.php");
    assert_eq!(entries.len(), 2);
    let by_name = |n: &str| entries.iter().find(|e| e.name == n).unwrap();
    assert_eq!(by_name("app:sync").aliases, vec!["app:s", "sync"]);
    assert_eq!(by_name("app:purge").aliases, vec!["app:p"]);
}

/// A leading `|` marks the command hidden; the name follows it, and the
/// reported offset must still point at the name inside the literal.
#[test]
fn leading_pipe_marks_hidden_without_swallowing_the_name() {
    let content =
        "<?php #[AsCommand(name: '|app:sync|app:s')] class SyncCommand extends Command {}";
    let entries = scan_command_file(content, "file:///app/Console/Commands/SyncCommand.php");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "app:sync");
    assert_eq!(entries[0].aliases, vec!["app:s"]);
    let offset = entries[0].name_offset as usize;
    assert_eq!(&content[offset..offset + "app:sync".len()], "app:sync");
}

/// A command that really *is* named `b:run` must win over one that merely
/// answers to `b:run` as an alias, whichever order the files hash into.
#[test]
fn primary_names_win_over_aliases() {
    let mut index = LaravelCommandIndex::default();
    index.files.set_file(
        "file:///a.php".to_string(),
        scan_command_file(
            "<?php #[Signature('a:run', aliases: ['b:run'])] class ARun extends Command { }",
            "file:///a.php",
        ),
    );
    index.files.set_file(
        "file:///b.php".to_string(),
        scan_command_file(
            "<?php class BCommand extends Command { protected $signature = 'b:run'; }",
            "file:///b.php",
        ),
    );
    index.rebuild();

    assert_eq!(index.get("b:run").unwrap().uri, "file:///b.php");
}

#[test]
fn index_dedupes_and_looks_up() {
    let mut index = LaravelCommandIndex::default();
    index.files.set_file(
        "file:///a.php".to_string(),
        scan_command_file(
            "<?php class ACommand extends Command { protected $signature = 'a:run {x}'; }",
            "file:///a.php",
        ),
    );
    index.files.set_file(
        "file:///b.php".to_string(),
        scan_command_file(
            "<?php class BCommand extends Command { protected $signature = 'b:run'; }",
            "file:///b.php",
        ),
    );
    index.rebuild();

    assert!(index.get("a:run").is_some());
    assert!(index.get("b:run").is_some());
    assert!(index.get("c:run").is_none());
    assert_eq!(index.all_names(), vec!["a:run", "b:run"]);
    assert_eq!(arg_names(&index.get("a:run").unwrap().signature), vec!["x"]);

    // Removing a file drops its command.
    index
        .files
        .set_file("file:///a.php".to_string(), Vec::new());
    index.rebuild();
    assert!(index.get("a:run").is_none());
    assert!(index.get("b:run").is_some());
}
