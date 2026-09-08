//! Regression test for the startup ordering between the Blueprint macro
//! scan and the Laravel schema/migration index.
//!
//! `load_schema_index` expands `$table->customHelper(...)` calls using the
//! project's `Blueprint::macro()` registrations, but at LSP startup those
//! registrations are only known once `build_laravel_macro_index` has run.
//! If the schema index loads first, it sees an empty macro map and the
//! columns a macro adds never make it onto the model — until an unrelated
//! edit forces a rebuild. This test drives the real `initialized()` startup
//! path (no `did_open` on the provider or migration, and no follow-up edit)
//! to prove the macro-added column is present from the very first load.

use crate::common::{create_psr4_workspace, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const PROVIDERS_PHP: &str = "\
<?php
return [
    App\\Providers\\AppServiceProvider::class,
];
";

const PROVIDER_PHP: &str = "\
<?php
namespace App\\Providers;
use Illuminate\\Database\\Schema\\Blueprint;
use Illuminate\\Support\\ServiceProvider;
class AppServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        Blueprint::macro('auditColumns', function () {
            $this->unsignedBigInteger('created_by')->nullable();
        });
    }
}
";

const DATABASE_CONFIG_PHP: &str = "\
<?php
return [
    'default' => 'mysql',
];
";

const MIGRATION_PHP: &str = "\
<?php
use Illuminate\\Database\\Migrations\\Migration;
use Illuminate\\Database\\Schema\\Blueprint;
use Illuminate\\Support\\Facades\\Schema;

return new class extends Migration {
    public function up(): void
    {
        Schema::create('orders', function (Blueprint $table): void {
            $table->id();
            $table->auditColumns();
            $table->string('status');
        });
    }
};
";

const ORDER_MODEL_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Order extends Model {}
";

const CONSUMER_PHP: &str = "\
<?php
namespace App;
use App\\Models\\Order;
class Consumer {
    public function go(Order $order): void {
        $order->
    }
}
";

#[tokio::test]
async fn blueprint_macro_column_is_present_from_the_first_startup_load() {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("config/database.php", DATABASE_CONFIG_PHP),
            ("src/Providers/AppServiceProvider.php", PROVIDER_PHP),
            (
                "database/migrations/2024_01_01_000000_create_orders_table.php",
                MIGRATION_PHP,
            ),
            ("src/Models/Order.php", ORDER_MODEL_PHP),
            ("src/Consumer.php", CONSUMER_PHP),
        ],
    );

    // The full startup sequence: this is what a real client triggers once,
    // before opening any file. Neither the provider nor the migration is
    // opened here, so nothing besides `initialized()` can have populated the
    // macro or schema index.
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Consumer.php")).unwrap();
    open_php(&backend, &uri, CONSUMER_PHP).await;

    let position = Position {
        line: 5,
        character: 16,
    };
    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    let items = match result.expect("completion should return results") {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let names: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        names.contains(&"created_by"),
        "the macro-added column should be a known property from the first \
         startup load, without editing the provider or migration, got: {names:?}"
    );
    assert!(
        names.contains(&"status"),
        "an ordinary migration column should still be present, got: {names:?}"
    );
}
