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
fn index_dedupes_and_looks_up() {
    let mut index = LaravelCommandIndex::default();
    index.set_file(
        "file:///a.php".to_string(),
        scan_command_file(
            "<?php class ACommand extends Command { protected $signature = 'a:run {x}'; }",
            "file:///a.php",
        ),
    );
    index.set_file(
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
    index.set_file("file:///a.php".to_string(), Vec::new());
    index.rebuild();
    assert!(index.get("a:run").is_none());
    assert!(index.get("b:run").is_some());
}
