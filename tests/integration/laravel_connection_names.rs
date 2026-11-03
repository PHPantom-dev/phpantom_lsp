//! Connection names selected through typed receivers and model/job properties.

use crate::common::{
    completion_response, create_psr4_workspace, definition_locations, goto_definition_at,
    hover_text, open_php, position_after, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{"require":{"laravel/framework":"^12.0"},"autoload":{"psr-4":{"App\\":"app/","Illuminate\\":"framework/"}}}"#;
const DATABASE: &str = "<?php return ['connections' => ['shared' => [], 'sqlite' => []]];";
const QUEUE: &str = "<?php return ['connections' => ['shared' => [], 'sync' => []]];";
const BROADCAST: &str = "<?php return ['connections' => ['shared' => [], 'reverb' => []]];";

async fn workspace(source: &str) -> (Backend, tempfile::TempDir, Uri) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER,
        &[
            ("config/database.php", DATABASE),
            ("config/queue.php", QUEUE),
            ("config/broadcasting.php", BROADCAST),
            (
                "framework/Database/ConnectionResolverInterface.php",
                "<?php namespace Illuminate\\Database; interface ConnectionResolverInterface { public function connection($name = null); }",
            ),
            (
                "framework/Database/DatabaseManager.php",
                "<?php namespace Illuminate\\Database; class DatabaseManager implements ConnectionResolverInterface { public function connection($name = null): object {} }",
            ),
            (
                "framework/Contracts/Queue/Factory.php",
                "<?php namespace Illuminate\\Contracts\\Queue; interface Factory { public function connection($name = null); }",
            ),
            (
                "framework/Contracts/Broadcasting/Factory.php",
                "<?php namespace Illuminate\\Contracts\\Broadcasting; interface Factory { public function connection($name = null); }",
            ),
            (
                "framework/Contracts/Queue/ShouldQueue.php",
                "<?php namespace Illuminate\\Contracts\\Queue; interface ShouldQueue {}",
            ),
            (
                "framework/Bus/Queueable.php",
                "<?php namespace Illuminate\\Bus; trait Queueable { public $connection; public function onConnection($connection): static { $this->connection = $connection; return $this; } }",
            ),
            (
                "framework/Foundation/Queue/Queueable.php",
                "<?php namespace Illuminate\\Foundation\\Queue; trait Queueable { use \\Illuminate\\Bus\\Queueable; }",
            ),
            (
                "framework/Foundation/Bus/PendingDispatch.php",
                "<?php namespace Illuminate\\Foundation\\Bus; class PendingDispatch { public function onConnection($connection): static { return $this; } }",
            ),
            (
                "framework/Foundation/Bus/PendingChain.php",
                "<?php namespace Illuminate\\Foundation\\Bus; class PendingChain { public function onConnection($connection): static { return $this; } }",
            ),
            (
                "framework/Database/Eloquent/Model.php",
                "<?php namespace Illuminate\\Database\\Eloquent; class Model { protected $connection; }",
            ),
            ("app/Connections.php", source),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(dir.path().join("app/Connections.php")).unwrap();
    open_php(&backend, &uri, source).await;
    (backend, dir, uri)
}

async fn labels_at(backend: &Backend, uri: &Uri, source: &str, needle: &str) -> Vec<String> {
    let pos = position_after(source, needle);
    response_labels(completion_response(backend, uri, pos.line, pos.character, None).await)
}

const SOURCE: &str = r#"<?php
namespace App;
use Illuminate\Database\ConnectionResolverInterface as Database;
use Illuminate\Contracts\Queue\Factory as Queue;
use Illuminate\Contracts\Broadcasting\Factory as Broadcast;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Foundation\Queue\Queueable;

class Customer extends Model {
    protected $connection = 'shared';
    public function change(): void { $this->connection = 'sqlite'; }
}
class JobBase { use Queueable; }
class Job extends JobBase {
    public function change(): void { $this->connection = 'sync'; }
}
class ContractJob implements \Illuminate\Contracts\Queue\ShouldQueue {
    public $connection = 'sync';
}
class Consumer {
    public function __construct(private Database $db) {}
    public function run(Queue $queue, Broadcast $broadcast, Job $job,
        \Illuminate\Foundation\Bus\PendingDispatch $pending,
        \Illuminate\Foundation\Bus\PendingChain $chain): void {
        $this->db->connection('shared');
        $queue->connection('shared');
        $broadcast->connection('shared');
        $job->onConnection('shared');
        $pending->onConnection(connection: 'shared');
        $chain->onConnection('shared');
        $queue?->connection(name: 'sync');
        $this->db->CONNECTION(name: 'sqlite::read');
        $job->onConnection('sync')->onConnection('sync');
        $manager = new \Illuminate\Database\DatabaseManager();
        $manager->connection('sqlite');
    }
}
"#;

#[tokio::test]
async fn typed_connections_complete_the_receiver_config_family() {
    let (backend, _dir, uri) = workspace(SOURCE).await;
    for (needle, expected) in [
        ("$this->db->connection('", vec!["shared", "sqlite"]),
        ("$queue->connection('", vec!["null", "shared", "sync"]),
        ("$broadcast->connection('", vec!["reverb", "shared"]),
        ("$job->onConnection('", vec!["null", "shared", "sync"]),
        (
            "$pending->onConnection(connection: '",
            vec!["null", "shared", "sync"],
        ),
        ("$chain->onConnection('", vec!["null", "shared", "sync"]),
        (
            "$queue?->connection(name: '",
            vec!["null", "shared", "sync"],
        ),
        (
            "$this->db->CONNECTION(name: 'sqlite::",
            vec!["sqlite::read", "sqlite::write", "sqlite::direct"],
        ),
        (
            "$job->onConnection('sync')->onConnection('",
            vec!["null", "shared", "sync"],
        ),
        ("$manager->connection('", vec!["shared", "sqlite"]),
        ("protected $connection = '", vec!["shared", "sqlite"]),
        ("$this->connection = 'sq", vec!["sqlite"]),
        ("$this->connection = 'sy", vec!["sync"]),
        ("public $connection = '", vec!["null", "shared", "sync"]),
    ] {
        assert_eq!(
            labels_at(&backend, &uri, SOURCE, needle).await,
            expected,
            "{needle}"
        );
    }
}

#[tokio::test]
async fn typed_connections_share_hover_definition_and_reference_identity() {
    let (backend, dir, uri) = workspace(SOURCE).await;
    for (needle, label, file, count) in [
        (
            "$this->db->connection('sha",
            "Database connection",
            "database.php",
            3,
        ),
        (
            "$queue->connection('sha",
            "Queue connection",
            "queue.php",
            5,
        ),
        (
            "$broadcast->connection('sha",
            "Broadcast connection",
            "broadcasting.php",
            2,
        ),
        (
            "protected $connection = 'sha",
            "Database connection",
            "database.php",
            3,
        ),
        (
            "$pending->onConnection(connection: 'sha",
            "Queue connection",
            "queue.php",
            5,
        ),
    ] {
        let position = position_after(SOURCE, needle);
        let hover = backend
            .handle_hover(uri.as_str(), SOURCE, position)
            .expect(needle);
        assert!(hover_text(&hover).contains(label), "{needle}: {hover:?}");
        let locations = definition_locations(
            goto_definition_at(&backend, &uri, position.line, position.character).await,
        );
        assert_eq!(locations.len(), 1, "{needle}: {locations:?}");
        assert_eq!(
            locations[0].uri,
            Uri::from_file_path(dir.path().join("config").join(file)).unwrap()
        );
        let refs = backend
            .find_references(uri.as_str(), SOURCE, position, true)
            .expect(needle);
        assert_eq!(refs.len(), count, "{needle}: {refs:?}");
    }
    for (needle, label) in [
        ("$queue?->connection(name: 'sy", "Queue connection"),
        ("$this->connection = 'sq", "Database connection"),
        ("$this->connection = 'sy", "Queue connection"),
        ("public $connection = 'sy", "Queue connection"),
    ] {
        let hover = backend
            .handle_hover(uri.as_str(), SOURCE, position_after(SOURCE, needle))
            .expect(needle);
        assert!(hover_text(&hover).contains(label), "{needle}: {hover:?}");
    }
}

#[tokio::test]
async fn typed_connections_diagnose_only_confirmed_literal_selections() {
    let source = format!(
        "{SOURCE}\nfunction invalid(Database $db, Queue $queue, Broadcast $broadcast, Job $job) {{\n$db->connection('sync');\n$queue->connection('sqlite');\n$broadcast->connection('sqlite');\n$job->onConnection('sqlite');\n$queue->connection('null');\n$db->connection('sqlite::read');\n$db->connection(null);\n$queue->connection($unknown);\n}}\nclass InvalidModel extends Model {{ protected $connection = 'sync'; }}\nclass InvalidJob extends Job {{ public function change(): void {{ $this->connection = 'sqlite'; }} }}"
    );
    let (backend, _dir, uri) = workspace(&source).await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), &source, &mut diagnostics);
    let invalid: Vec<_> = diagnostics
        .iter()
        .filter_map(|d| match &d.code {
            Some(Code::String(code)) if code.starts_with("invalid_laravel_") => Some(code.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        invalid
            .iter()
            .filter(|code| **code == "invalid_laravel_database_connection")
            .count(),
        2,
        "{diagnostics:?}"
    );
    assert_eq!(
        invalid
            .iter()
            .filter(|code| **code == "invalid_laravel_queue_connection")
            .count(),
        3,
        "{diagnostics:?}"
    );
    assert_eq!(
        invalid
            .iter()
            .filter(|code| **code == "invalid_laravel_broadcast_connection")
            .count(),
        1,
        "{diagnostics:?}"
    );
    assert_eq!(invalid.len(), 6);
}

#[tokio::test]
async fn typed_connections_ignore_homonyms_and_other_argument_shapes() {
    let source = r#"<?php
namespace App;
class DatabaseManager {
    public $connection = 'shared';
    public static $other = 'shared';
    public function connection($name = null) {}
    public function onConnection($connection) {}
    public function run(): void {
        $this->connection('shared');
        $this->onConnection('shared');
        $this->connection = 'shared';
        $connection = 'shared';
    }
}

function run(\Illuminate\Database\ConnectionResolverInterface $db) {
    $db->connection(wrong: 'shared');
    $db->connection('sqlite', 'shared');
    $db->connection(['shared']);
    $db->connection('sql' . 'shared');
    $db->connection(...['shared']);
}
class StaticModel extends \Illuminate\Database\Eloquent\Model { public static $connection = 'shared'; }
"#;
    let (backend, _dir, uri) = workspace(source).await;
    for (offset, _) in source.match_indices("shared") {
        let position = crate::common::offset_position(source, offset + 2);
        let labels = response_labels(
            completion_response(&backend, &uri, position.line, position.character, None).await,
        );
        assert!(
            !labels.iter().any(|label| label == "shared"),
            "{offset}: {labels:?}"
        );
        let hover = backend.handle_hover(uri.as_str(), source, position);
        assert!(
            hover
                .as_ref()
                .is_none_or(|h| !hover_text(h).contains("connection**")),
            "{offset}: {hover:?}"
        );
    }
}

#[tokio::test]
async fn typed_connections_references_include_config_facades_and_other_files() {
    let (backend, dir, uri) = workspace(SOURCE).await;
    let other = r#"<?php
namespace Other;
use Illuminate\Database\ConnectionResolverInterface as Factory;
use Illuminate\Support\Facades\DB;
function consume(Factory $manager): void {
    $manager->connection('shared::read');
    DB::connection('shared');
    config('database.connections.shared');
}
"#;
    let path = dir.path().join("app/Other.php");
    std::fs::write(&path, other).unwrap();
    let other_uri = Uri::from_file_path(path).unwrap();
    open_php(&backend, &other_uri, other).await;
    for (file_uri, content, needle) in [
        (&uri, SOURCE, "$this->db->connection('sha"),
        (&other_uri, other, "$manager->connection('sha"),
        (&other_uri, other, "config('database.connections.sha"),
    ] {
        let references = backend
            .find_references(
                file_uri.as_str(),
                content,
                position_after(content, needle),
                false,
            )
            .unwrap();
        assert_eq!(references.len(), 5, "{needle}: {references:?}");
        assert_eq!(
            references
                .iter()
                .filter(|location| location.uri == other_uri)
                .count(),
            3
        );
    }
    let config_uri = Uri::from_file_path(dir.path().join("config/database.php")).unwrap();
    let references = backend
        .find_references(
            config_uri.as_str(),
            DATABASE,
            position_after(DATABASE, "'sha"),
            true,
        )
        .unwrap();
    assert_eq!(references.len(), 6, "{references:?}");
}

#[tokio::test]
async fn typed_connections_follow_nullable_and_narrowed_receivers() {
    let source = r#"<?php
namespace App;
use Illuminate\Database\ConnectionResolverInterface as Database;
use Illuminate\Contracts\Queue\Factory as Queue;
function consume(?Database $nullable, Database|Queue $ambiguous): void {
    $nullable?->connection('shared');
    $ambiguous->connection('shared');
    if ($ambiguous instanceof Queue) {
        $ambiguous->connection('sync');
    }
    $nullable = new \Illuminate\Database\DatabaseManager();
    $nullable->connection('sqlite');
}
"#;
    let (backend, _dir, uri) = workspace(source).await;
    for (needle, expected) in [
        ("$nullable?->connection('", vec!["shared", "sqlite"]),
        ("$ambiguous->connection('sy", vec!["sync"]),
        ("$nullable->connection('", vec!["shared", "sqlite"]),
    ] {
        assert_eq!(
            labels_at(&backend, &uri, source, needle).await,
            expected,
            "{needle}"
        );
        assert!(
            backend
                .handle_hover(uri.as_str(), source, position_after(source, needle))
                .is_some()
        );
    }
    let ambiguous = position_after(source, "$ambiguous->connection('sha");
    assert!(
        !labels_at(&backend, &uri, source, "$ambiguous->connection('sha")
            .await
            .contains(&"shared".to_string())
    );
    assert!(
        backend
            .handle_hover(uri.as_str(), source, ambiguous)
            .is_none()
    );
}

#[tokio::test]
async fn typed_connections_complete_unfinished_literals_and_refresh_receiver_types() {
    for (source, expected) in [
        (
            "<?php function run(\\Illuminate\\Contracts\\Queue\\Factory $queue) { $queue->connection(name: '",
            vec!["null", "shared", "sync"],
        ),
        (
            "<?php class Customer extends \\Illuminate\\Database\\Eloquent\\Model { protected $connection = '",
            vec!["shared", "sqlite"],
        ),
    ] {
        let (backend, _dir, uri) = workspace(source).await;
        assert_eq!(
            labels_at(&backend, &uri, source, source).await,
            expected,
            "{source}"
        );
    }
    let source = "<?php function run(\\Illuminate\\Contracts\\Queue\\Factory $factory) { $factory->connection('shared'); }";
    let (backend, _dir, uri) = workspace(source).await;
    let position = position_after(source, "connection('sha");
    assert!(
        hover_text(
            &backend
                .handle_hover(uri.as_str(), source, position)
                .unwrap()
        )
        .contains("Queue connection")
    );
    let changed = source.replace(
        "Contracts\\Queue\\Factory",
        "Contracts\\Broadcasting\\Factory",
    );
    open_php(&backend, &uri, &changed).await;
    let position = position_after(&changed, "connection('sha");
    assert!(
        hover_text(
            &backend
                .handle_hover(uri.as_str(), &changed, position)
                .unwrap()
        )
        .contains("Broadcast connection")
    );
}

#[tokio::test]
async fn typed_connections_cover_queue_dispatch_forms_and_hooked_properties() {
    let source = r#"<?php
namespace App;
use Illuminate\Contracts\Queue\ShouldQueue;
class HookedJob implements ShouldQueue {
    public string $connection = 'sync' { get => $this->connection; }
}
function queued(\Illuminate\Bus\PendingBatch $batch, \Illuminate\Events\QueuedClosure $listener): void {
    $batch->onConnection('shared');
    $listener->onConnection('sync');
    $listener->connection = 'sync';
}
"#;
    let (backend, dir, uri) = workspace(source).await;
    for (relative, stub) in [
        (
            "framework/Bus/PendingBatch.php",
            "<?php namespace Illuminate\\Bus; class PendingBatch { public function onConnection($connection): static { return $this; } }",
        ),
        (
            "framework/Events/QueuedClosure.php",
            "<?php namespace Illuminate\\Events; class QueuedClosure { public $connection; public function onConnection($connection): static { return $this; } }",
        ),
    ] {
        let path = dir.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, stub).unwrap();
        open_php(&backend, &Uri::from_file_path(path).unwrap(), stub).await;
    }
    for needle in [
        "public string $connection = '",
        "$batch->onConnection('",
        "$listener->onConnection('",
        "$listener->connection = '",
    ] {
        assert_eq!(
            labels_at(&backend, &uri, source, needle).await,
            ["null", "shared", "sync"],
            "{needle}"
        );
        let hover = backend
            .handle_hover(uri.as_str(), source, position_after(source, needle))
            .unwrap();
        assert!(hover_text(&hover).contains("Queue connection"));
    }
}

#[tokio::test]
async fn typed_connections_do_not_diagnose_unreadable_config_families() {
    let source = "<?php function run(\\Illuminate\\Contracts\\Queue\\Factory $queue) { $queue->connection('dynamic'); }";
    let (backend, dir, uri) = workspace(source).await;
    let path = dir.path().join("config/queue.php");
    std::fs::remove_file(&path).unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(&path).unwrap(),
                kind: FileChangeType::Deleted,
            }],
        })
        .await;
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code != Some(Code::String("invalid_laravel_queue_connection".into()))),
        "{diagnostics:?}"
    );
    std::fs::write(&path, "<?php return ['connections' => ['dynamic' => []]];").unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: Uri::from_file_path(path).unwrap(),
                kind: FileChangeType::Created,
            }],
        })
        .await;
    assert_eq!(
        labels_at(&backend, &uri, source, "connection('").await,
        ["dynamic", "null"]
    );
}

#[tokio::test]
async fn typed_connections_are_gated_on_laravel_projects() {
    let source = "<?php function run(\\Illuminate\\Contracts\\Queue\\Factory $queue) { $queue->connection('sync'); }";
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"app/"}}}"#,
        &[("config/queue.php", QUEUE), ("app/Plain.php", source)],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Uri::from_file_path(dir.path().join("app/Plain.php")).unwrap();
    open_php(&backend, &uri, source).await;
    assert!(
        !labels_at(&backend, &uri, source, "connection('")
            .await
            .contains(&"sync".to_string())
    );
    assert!(
        backend
            .handle_hover(
                uri.as_str(),
                source,
                position_after(source, "connection('sy")
            )
            .is_none()
    );
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), source, &mut diagnostics);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.code != Some(Code::String("invalid_laravel_queue_connection".into())))
    );
}

#[tokio::test]
async fn typed_connections_recover_nested_anonymous_model_defaults() {
    let source = "<?php save([new class extends \\Illuminate\\Database\\Eloquent\\Model { protected $connection = '";
    let (backend, _dir, uri) = workspace(source).await;
    assert_eq!(
        labels_at(&backend, &uri, source, source).await,
        ["shared", "sqlite"]
    );
}

#[tokio::test]
async fn typed_connections_ignore_constants_and_static_calls_during_edits() {
    let source = r#"<?php
namespace App;
class Settings { const connection = 'shared'; }
Settings::connection('shared');
Settings::connection = 'shared';
"#;
    let (backend, _dir, uri) = workspace(source).await;
    for needle in [
        "const connection = '",
        "Settings::connection('",
        "Settings::connection = '",
    ] {
        assert!(
            !labels_at(&backend, &uri, source, needle)
                .await
                .contains(&"shared".to_string()),
            "{needle}"
        );
    }
}
