//! Code lenses linking a Laravel class to the classes and files it is paired
//! with: a model's factory, policy, observers, migrations, resources and
//! collection, the way back from a factory, policy or observer, and the
//! links between an event and its listeners.

use crate::common::{create_psr4_workspace, open_initialized_php, references_response};
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const DATABASE_CONFIG: &str = "<?php\nreturn ['default' => 'mysql'];\n";

const PROVIDERS: &str = "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n";

const APP_SERVICE_PROVIDER: &str = r#"<?php
namespace App\Providers;

use App\Events\OrderShipped;
use App\Listeners\AuditOrders;
use App\Listeners\SendShipmentNotification;
use App\Models\Post;
use App\Observers\AuditObserver;
use Illuminate\Support\Facades\Event;

class AppServiceProvider
{
    public function boot(): void
    {
        Post::observe(AuditObserver::class);
        Event::listen(OrderShipped::class, [AuditOrders::class, 'record']);
        Event::listen(OrderShipped::class, SendShipmentNotification::class);
    }
}
"#;

const POST: &str = r#"<?php
namespace App\Models;

use App\Collections\PostCollection;
use App\Observers\PostObserver;
use Illuminate\Database\Eloquent\Attributes\CollectedBy;
use Illuminate\Database\Eloquent\Attributes\ObservedBy;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;

#[ObservedBy([PostObserver::class])]
#[CollectedBy(PostCollection::class)]
class Post extends Model
{
    use HasFactory;
}
"#;

const POST_FACTORY: &str = r#"<?php
namespace Database\Factories;

use Illuminate\Database\Eloquent\Factories\Factory;

class PostFactory extends Factory
{
    public function definition(): array
    {
        return [];
    }
}
"#;

const POST_POLICY: &str = r#"<?php
namespace App\Policies;

class PostPolicy
{
    public function update(): bool
    {
        return true;
    }
}
"#;

const POST_OBSERVER: &str = r#"<?php
namespace App\Observers;

class PostObserver
{
    public function created(): void {}
}
"#;

const AUDIT_OBSERVER: &str = r#"<?php
namespace App\Observers;

class AuditObserver
{
    public function saved(): void {}
}
"#;

const POST_RESOURCE: &str = r#"<?php
namespace App\Http\Resources;

class PostResource {}
"#;

const POST_COLLECTION: &str = r#"<?php
namespace App\Collections;

use Illuminate\Database\Eloquent\Collection;

class PostCollection extends Collection {}
"#;

const CREATE_POSTS: &str = r#"<?php
use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::create('posts', function (Blueprint $table) {
            $table->id();
        });
    }
};
"#;

const ALTER_POSTS: &str = r#"<?php
use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

return new class extends Migration
{
    public function up(): void
    {
        Schema::table('posts', function (Blueprint $table) {
            $table->string('title');
        });
    }
};
"#;

const ORDER_SHIPPED: &str = r#"<?php
namespace App\Events;

class OrderShipped implements ShouldNotify {}
"#;

const SHOULD_NOTIFY: &str = r#"<?php
namespace App\Events;

interface ShouldNotify {}
"#;

const SEND_SHIPMENT_NOTIFICATION: &str = r#"<?php
namespace App\Listeners;

use App\Events\OrderShipped;

class SendShipmentNotification
{
    public function handle(OrderShipped $event): void {}
}
"#;

const NOTIFY_ANYTHING: &str = r#"<?php
namespace App\Listeners;

use App\Events\ShouldNotify;

class NotifyAnything
{
    public function handle(ShouldNotify $event): void {}
}
"#;

const AUDIT_ORDERS: &str = r#"<?php
namespace App\Listeners;

class AuditOrders
{
    public function record($event): void {}
}
"#;

/// A shared base that, like Eloquent's own `Model`, names the default
/// collection, which is no customisation to link to.
const BASE_MODEL: &str = r#"<?php
namespace App\Models;

use Illuminate\Database\Eloquent\HasCollection;
use Illuminate\Database\Eloquent\Model;

abstract class BaseModel extends Model
{
    /** @use HasCollection<\Illuminate\Database\Eloquent\Collection<array-key, static>> */
    use HasCollection;
}
"#;

const COMMENT: &str = r#"<?php
namespace App\Models;

class Comment extends BaseModel {}
"#;

/// Eloquent's own collection, loadable so that a link to it would show.
const ELOQUENT_COLLECTION: &str = r#"<?php
namespace Illuminate\Database\Eloquent;

class Collection {}
"#;

const FILES: &[(&str, &str)] = &[
    ("framework/Eloquent/Collection.php", ELOQUENT_COLLECTION),
    ("app/Models/BaseModel.php", BASE_MODEL),
    ("app/Models/Comment.php", COMMENT),
    ("config/database.php", DATABASE_CONFIG),
    ("bootstrap/providers.php", PROVIDERS),
    ("app/Providers/AppServiceProvider.php", APP_SERVICE_PROVIDER),
    ("app/Models/Post.php", POST),
    ("database/factories/PostFactory.php", POST_FACTORY),
    ("app/Policies/PostPolicy.php", POST_POLICY),
    ("app/Observers/PostObserver.php", POST_OBSERVER),
    ("app/Observers/AuditObserver.php", AUDIT_OBSERVER),
    ("app/Http/Resources/PostResource.php", POST_RESOURCE),
    ("app/Collections/PostCollection.php", POST_COLLECTION),
    (
        "database/migrations/2024_01_01_000000_create_posts_table.php",
        CREATE_POSTS,
    ),
    (
        "database/migrations/2024_02_01_000000_add_title_to_posts_table.php",
        ALTER_POSTS,
    ),
    ("app/Events/OrderShipped.php", ORDER_SHIPPED),
    ("app/Events/ShouldNotify.php", SHOULD_NOTIFY),
    (
        "app/Listeners/SendShipmentNotification.php",
        SEND_SHIPMENT_NOTIFICATION,
    ),
    ("app/Listeners/NotifyAnything.php", NOTIFY_ANYTHING),
    ("app/Listeners/AuditOrders.php", AUDIT_ORDERS),
];

const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": {
        "App\\": "app/",
        "Database\\Factories\\": "database/factories/",
        "Illuminate\\Database\\Eloquent\\": "framework/Eloquent/"
    } }
}"#;

/// A related-file lens: its line, title, and the targets it goes to as
/// (file name, line) pairs.
type Lens = (u32, String, Vec<(String, u32)>);

/// The related-file lenses on the file at `path`, once the workspace is
/// indexed.  Reference and implementation lenses are left out.
async fn related_lenses(path: &str) -> Vec<Lens> {
    let (backend, _dir) = create_psr4_workspace(COMPOSER, FILES);
    let uri = open_initialized_php(&backend, path).await;
    // Drive workspace indexing the way a client does before asking for lenses.
    references_response(&backend, &uri, Position::new(0, 0), false).await;
    backend
        .code_lens(CodeLensParams {
            text_document: TextDocumentIdentifier::new(uri),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|lens| {
            let command = lens.command?;
            let title = command.title.trim_end_matches('s');
            if title.ends_with("reference") || title.ends_with("implementation") {
                return None;
            }
            let targets = targets(&command);
            Some((lens.range.start.line, command.title, targets))
        })
        .collect()
}

/// The (file name, line) pairs a lens command goes to, whichever of the two
/// command shapes it uses.
fn targets(command: &Command) -> Vec<(String, u32)> {
    let arguments = command.arguments.as_deref().unwrap_or_default();
    let target = |uri: &serde_json::Value, line: &serde_json::Value| {
        let uri = uri.as_str()?;
        let name = uri.rsplit('/').next()?.to_string();
        Some((name, line.as_u64()? as u32))
    };
    if command.command == "editor.action.showReferences" {
        arguments
            .get(2)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|location| target(&location["uri"], &location["range"]["start"]["line"]))
            .collect()
    } else {
        arguments
            .first()
            .and_then(|argument| target(&argument["uri"], &argument["position"]["line"]))
            .into_iter()
            .collect()
    }
}

fn titles(lenses: &[Lens]) -> Vec<&str> {
    lenses.iter().map(|(_, title, _)| title.as_str()).collect()
}

#[tokio::test]
async fn a_model_links_to_its_factory_policy_observers_migrations_and_resources() {
    let lenses = related_lenses("app/Models/Post.php").await;
    let find = |prefix: &str| {
        lenses
            .iter()
            .find(|(_, title, _)| title.starts_with(prefix))
            .unwrap_or_else(|| panic!("no `{prefix}` lens in {:?}", titles(&lenses)))
    };

    assert_eq!(find("Factory").1, "Factory: PostFactory");
    assert_eq!(find("Factory").2, vec![("PostFactory.php".to_string(), 5)]);
    assert_eq!(find("Policy").1, "Policy: PostPolicy");
    assert_eq!(find("Resource:").1, "Resource: PostResource");
    assert_eq!(find("Collection").1, "Collection: PostCollection");
    assert_eq!(find("2 observers").2.len(), 2);
    assert_eq!(
        find("2 migrations").2,
        vec![
            ("2024_01_01_000000_create_posts_table.php".to_string(), 9),
            (
                "2024_02_01_000000_add_title_to_posts_table.php".to_string(),
                9
            ),
        ]
    );
    // Every lens sits on the class declaration line.
    assert!(lenses.iter().all(|(line, _, _)| *line == 12), "{lenses:?}");
}

#[tokio::test]
async fn a_factory_policy_and_observer_link_back_to_the_model() {
    for path in [
        "database/factories/PostFactory.php",
        "app/Policies/PostPolicy.php",
        "app/Observers/PostObserver.php",
        "app/Observers/AuditObserver.php",
    ] {
        let lenses = related_lenses(path).await;
        assert_eq!(titles(&lenses), vec!["Model: Post"], "{path}");
        assert_eq!(lenses[0].2, vec![("Post.php".to_string(), 12)], "{path}");
    }
}

#[tokio::test]
async fn an_event_links_to_the_listeners_that_handle_it() {
    let lenses = related_lenses("app/Events/OrderShipped.php").await;
    assert_eq!(titles(&lenses), vec!["3 listeners"]);
    let mut targets = lenses[0].2.clone();
    targets.sort();
    assert_eq!(
        targets,
        vec![
            ("AuditOrders.php".to_string(), 5),
            ("NotifyAnything.php".to_string(), 7),
            ("SendShipmentNotification.php".to_string(), 7),
        ]
    );
}

#[tokio::test]
async fn a_listener_links_to_the_event_it_handles() {
    let lenses = related_lenses("app/Listeners/SendShipmentNotification.php").await;
    assert_eq!(
        lenses,
        vec![(
            7,
            "Event: OrderShipped".to_string(),
            vec![("OrderShipped.php".to_string(), 3)]
        )]
    );

    let lenses = related_lenses("app/Listeners/AuditOrders.php").await;
    assert_eq!(
        lenses,
        vec![(
            5,
            "Event: OrderShipped".to_string(),
            vec![("OrderShipped.php".to_string(), 3)]
        )]
    );
}

#[tokio::test]
async fn a_model_with_the_default_collection_links_to_no_collection() {
    let lenses = related_lenses("app/Models/Comment.php").await;
    assert!(
        !titles(&lenses)
            .iter()
            .any(|title| title.starts_with("Collection")),
        "{lenses:?}"
    );
}
