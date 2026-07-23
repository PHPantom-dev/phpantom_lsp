use super::*;

fn ctx_at(content: &str, needle: &str) -> Option<DetectedContext> {
    // Place the cursor right after `needle` (which should end inside a quote).
    let idx = content.find(needle).expect("needle not found") + needle.len();
    let position = crate::util::offset_to_position(content, idx);
    detect_context(content, position)
}

#[test]
fn detects_own_argument() {
    let content = "<?php\n$this->argument('us');\n";
    let d = ctx_at(content, "argument('us").expect("should detect");
    assert!(matches!(d.context, ParamContext::OwnArgument));
    assert_eq!(d.prefix, "us");
}

#[test]
fn detects_own_option() {
    let content = "<?php\n$this->option('qu');\n";
    let d = ctx_at(content, "option('qu").expect("should detect");
    assert!(matches!(d.context, ParamContext::OwnOption));
    assert_eq!(d.prefix, "qu");
}

#[test]
fn ignores_unrelated_method() {
    let content = "<?php\n$this->foo('bar');\n";
    assert!(ctx_at(content, "foo('bar").is_none());
}

#[test]
fn detects_call_array_key_first() {
    let content = "<?php\nArtisan::call('app:sync', ['us']);\n";
    let d = ctx_at(content, "['us").expect("should detect array key");
    match d.context {
        ParamContext::CallArrayKey { command_name } => assert_eq!(command_name, "app:sync"),
        _ => panic!("expected CallArrayKey"),
    }
    assert_eq!(d.prefix, "us");
}

#[test]
fn detects_call_array_key_subsequent() {
    let content = "<?php\nArtisan::call('app:sync', ['user' => 1, '--qu']);\n";
    let d = ctx_at(content, "'--qu").expect("should detect subsequent key");
    match d.context {
        ParamContext::CallArrayKey { command_name } => assert_eq!(command_name, "app:sync"),
        _ => panic!("expected CallArrayKey"),
    }
    assert_eq!(d.prefix, "--qu");
}

#[test]
fn detects_this_call_array_key() {
    let content = "<?php\n$this->call('app:sync', ['us']);\n";
    let d = ctx_at(content, "['us").expect("should detect");
    match d.context {
        ParamContext::CallArrayKey { command_name } => assert_eq!(command_name, "app:sync"),
        _ => panic!("expected CallArrayKey"),
    }
}

#[test]
fn plain_array_not_command_call() {
    let content = "<?php\nfoo('x', ['us']);\n";
    assert!(ctx_at(content, "['us").is_none());
}

#[test]
fn own_argument_completion_end_to_end() {
    let backend = crate::Backend::new_test();
    let content = "<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class SyncCommand extends Command
{
    protected $signature = 'app:sync {user} {team?} {--queue}';
    public function handle(): void
    {
        $this->argument('');
    }
}
";
    let idx = content.find("argument('").unwrap() + "argument('".len();
    let position = crate::util::offset_to_position(content, idx);
    let response = backend.try_command_param_completion(content, position);
    let labels = collect_labels(response);
    assert!(labels.contains(&"user".to_string()), "got {labels:?}");
    assert!(labels.contains(&"team".to_string()), "got {labels:?}");
    // Options should not appear in argument() completion.
    assert!(!labels.contains(&"queue".to_string()), "got {labels:?}");
}

#[test]
fn own_option_completion_end_to_end() {
    let backend = crate::Backend::new_test();
    let content = "<?php
namespace App\\Console\\Commands;
use Illuminate\\Console\\Command;
class SyncCommand extends Command
{
    protected $signature = 'app:sync {user} {--queue} {--conn=}';
    public function handle(): void
    {
        $this->option('');
    }
}
";
    let idx = content.find("option('").unwrap() + "option('".len();
    let position = crate::util::offset_to_position(content, idx);
    let response = backend.try_command_param_completion(content, position);
    let labels = collect_labels(response);
    assert!(labels.contains(&"queue".to_string()), "got {labels:?}");
    assert!(labels.contains(&"conn".to_string()), "got {labels:?}");
    assert!(!labels.contains(&"user".to_string()), "got {labels:?}");
}

fn collect_labels(response: Option<CompletionResponse>) -> Vec<String> {
    match response {
        Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
        Some(CompletionResponse::List(list)) => list.items.into_iter().map(|i| i.label).collect(),
        None => Vec::new(),
    }
}
