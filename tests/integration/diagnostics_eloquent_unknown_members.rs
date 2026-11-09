//! Unknown-member diagnostics on Eloquent models and builders.
//!
//! Eloquent's `__call` accepts a closed set of names (the builder's methods,
//! scopes, macros and `where{Column}` calls) and throws for the rest, so a
//! name outside that set is reported even though the class has `__call`.
//! With `Model::preventAccessingMissingAttributes()` turned on, the same
//! holds for an unknown attribute.

use crate::common::{create_initialized_psr4_workspace, message_text};
use tower_lsp_server::gen_lsp_types::Diagnostic;

const COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "app/", "Illuminate\\": "illuminate/" } }
}"#;

const MODEL_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
use Illuminate\Database\Eloquent\Relations\HasMany;
abstract class Model {
    /** @return \Illuminate\Database\Eloquent\Builder<static> */
    public static function query() {}
    /** @return \Illuminate\Database\Eloquent\Builder<static> */
    public function newQuery() {}
    public function save(): bool { return true; }
    /**
     * @template TRelatedModel of Model
     * @param class-string<TRelatedModel> $related
     * @return HasMany<TRelatedModel, $this>
     */
    public function hasMany($related) {}
    public static function preventAccessingMissingAttributes($value = true) {}
    public static function resolveRelationUsing($name, $callback) {}
    public function __get($key) {}
    public function __set($key, $value) {}
    public function __call($method, $parameters) {}
    public static function __callStatic($method, $parameters) {}
}
"#;

const BUILDER_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
/**
 * @template TModel of \Illuminate\Database\Eloquent\Model
 * @mixin \Illuminate\Database\Query\Builder
 */
class Builder {
    /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
    public function get($columns = ['*']) {}
    /** @return TModel|null */
    public function first($columns = ['*']) {}
    public static function macro($name, $macro) {}
    public function __call($method, $parameters) {}
    public static function __callStatic($method, $parameters) {}
}
"#;

const COLLECTION_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
/**
 * @template TKey of array-key
 * @template TModel
 */
class Collection {}
"#;

const HAS_MANY_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent\Relations;
/**
 * @template TRelatedModel of \Illuminate\Database\Eloquent\Model
 * @template TDeclaringModel of \Illuminate\Database\Eloquent\Model
 */
class HasMany {}
"#;

const QUERY_BUILDER_PHP: &str = r#"<?php
namespace Illuminate\Database\Query;
class Builder {
    /** @return $this */
    public function where($column, $operator = null, $value = null) {}
    /** @return $this */
    public function orderBy($column, $direction = 'asc') {}
    public function count($columns = '*'): int { return 0; }
    public function __call($method, $parameters) {}
}
"#;

const SOFT_DELETES_PHP: &str = r#"<?php
namespace Illuminate\Database\Eloquent;
/**
 * @method static \Illuminate\Database\Eloquent\Builder<static> withTrashed(bool $withTrashed = true)
 */
trait SoftDeletes {
    public function restore(): bool { return true; }
}
"#;

const ORDER_PHP: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\SoftDeletes;
class Order extends Model {
    use SoftDeletes;
    public function total(): int { return 0; }
    public function scopePaid(Builder $query): void {}
    /** @return HasMany<Line, $this> */
    public function lines(): HasMany { return $this->hasMany(Line::class); }
}
"#;

const LINE_PHP: &str = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Line extends Model {}
"#;

/// The framework stubs and the two models every case shares.
fn framework() -> Vec<(&'static str, &'static str)> {
    vec![
        ("illuminate/Database/Eloquent/Model.php", MODEL_PHP),
        ("illuminate/Database/Eloquent/Builder.php", BUILDER_PHP),
        (
            "illuminate/Database/Eloquent/Collection.php",
            COLLECTION_PHP,
        ),
        (
            "illuminate/Database/Eloquent/SoftDeletes.php",
            SOFT_DELETES_PHP,
        ),
        (
            "illuminate/Database/Eloquent/Relations/HasMany.php",
            HAS_MANY_PHP,
        ),
        ("illuminate/Database/Query/Builder.php", QUERY_BUILDER_PHP),
        ("app/Models/Order.php", ORDER_PHP),
        ("app/Models/Line.php", LINE_PHP),
    ]
}

/// Lay out the framework, `extra` files and `consumer` at `app/Consumer.php`,
/// run the startup scans, and collect the consumer's unknown-member
/// diagnostics.
async fn diagnostics(extra: &[(&str, &str)], consumer: &str) -> Vec<Diagnostic> {
    let mut files = framework();
    files.extend_from_slice(extra);
    files.push(("app/Consumer.php", consumer));
    let (backend, _dir, uri) =
        create_initialized_psr4_workspace(COMPOSER, &files, "app/Consumer.php").await;
    let mut out = Vec::new();
    backend.collect_unknown_member_diagnostics(uri.as_str(), consumer, &mut out);
    out
}

fn messages(diags: &[Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .map(|d| message_text(&d.message).to_string())
        .collect()
}

/// Wrap `body` in a method of `App\Consumer` that receives an `Order`.
fn consumer(body: &str) -> String {
    format!(
        "<?php
namespace App;
use App\\Models\\Order;
use Illuminate\\Database\\Eloquent\\Builder;
use Illuminate\\Database\\Eloquent\\Model;
class Consumer {{
    public function go(Order $order, Model $model, Builder $builder): void {{
{body}
    }}
}}
"
    )
}

#[tokio::test]
async fn reports_unknown_method_on_model_instance() {
    let diags = diagnostics(&[], &consumer("        $order->getTotl();")).await;
    assert_eq!(
        messages(&diags),
        vec!["Method App\\Models\\Order::getTotl() not found"],
    );
}

#[tokio::test]
async fn reports_unknown_static_method_on_model() {
    let diags = diagnostics(&[], &consumer("        Order::findPaid();")).await;
    assert_eq!(
        messages(&diags),
        vec!["Method App\\Models\\Order::findPaid() not found"],
    );
}

#[tokio::test]
async fn reports_unknown_method_on_model_builder() {
    let diags = diagnostics(&[], &consumer("        Order::query()->paidd();")).await;
    assert_eq!(
        messages(&diags),
        vec!["Method Illuminate\\Database\\Eloquent\\Builder::paidd() not found"],
    );
}

#[tokio::test]
async fn accepts_names_eloquent_forwards() {
    let body = "\
        $order->total();
        $order->where('status', 'paid');
        $order->whereStatsu('paid');
        Order::whereAnything(1);
        Order::paid();
        Order::query()->paid()->orderBy('id')->count();
        Order::query()->whereStatus('paid')->get();
        $order->newQuery()->restore();
        Order::withTrashed()->get();
        $order->throughLines();";
    let diags = diagnostics(&[], &consumer(body)).await;
    assert!(diags.is_empty(), "got: {:?}", messages(&diags));
}

#[tokio::test]
async fn stays_silent_when_the_model_is_not_known() {
    let body = "\
        $model->anything();
        $builder->anyScope();";
    let diags = diagnostics(&[], &consumer(body)).await;
    assert!(diags.is_empty(), "got: {:?}", messages(&diags));
}

#[tokio::test]
async fn stays_silent_when_the_model_overrides_call() {
    let custom = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Model;
class Flexible extends Model {
    public function __call($method, $parameters) {}
}
"#;
    let body = "        (new \\App\\Models\\Flexible())->anything();";
    let diags = diagnostics(&[("app/Models/Flexible.php", custom)], &consumer(body)).await;
    assert!(diags.is_empty(), "got: {:?}", messages(&diags));
}

#[tokio::test]
async fn stays_silent_when_a_custom_builder_overrides_call() {
    let builder = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Builder;
/**
 * @template TModel of \App\Models\Ticket
 * @extends Builder<TModel>
 */
class TicketBuilder extends Builder {
    public function __call($method, $parameters) {}
}
"#;
    let model = r#"<?php
namespace App\Models;
use Illuminate\Database\Eloquent\Attributes\UseEloquentBuilder;
use Illuminate\Database\Eloquent\Model;
#[UseEloquentBuilder(TicketBuilder::class)]
class Ticket extends Model {}
"#;
    let body = "\
        \\App\\Models\\Ticket::query()->anything();
        (new \\App\\Models\\Ticket())->anything();";
    let diags = diagnostics(
        &[
            ("app/Models/TicketBuilder.php", builder),
            ("app/Models/Ticket.php", model),
        ],
        &consumer(body),
    )
    .await;
    assert!(diags.is_empty(), "got: {:?}", messages(&diags));
}

const PROVIDERS_PHP: &str = "<?php\nreturn [\n    App\\Providers\\AppServiceProvider::class,\n];\n";

/// An `AppServiceProvider` whose `boot()` runs `body`.
fn provider(body: &str) -> String {
    format!(
        "<?php
namespace App\\Providers;
use App\\Models\\Order;
use Illuminate\\Database\\Eloquent\\Builder;
use Illuminate\\Database\\Eloquent\\Model;
class AppServiceProvider {{
    public function boot(): void {{
{body}
    }}
}}
"
    )
}

#[tokio::test]
async fn accepts_names_registered_in_ways_the_scan_cannot_type() {
    let provider = provider(
        "\
        Builder::macro('fromCallable', [Helpers::class, 'run']);
        Order::resolveRelationUsing('customer', fn (Order $order) => null);",
    );
    let body = "\
        Order::query()->fromCallable();
        $order->fromCallable();
        $order->customer();
        $order->stillUnknown();";
    let diags = diagnostics(
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("app/Providers/AppServiceProvider.php", &provider),
        ],
        &consumer(body),
    )
    .await;
    assert_eq!(
        messages(&diags),
        vec!["Method App\\Models\\Order::stillUnknown() not found"],
    );
}

#[tokio::test]
async fn stays_silent_when_a_macro_name_cannot_be_read() {
    let provider = provider(
        "\
        foreach (['a', 'b'] as $name) {
            Builder::macro($name, fn () => $this);
        }",
    );
    let body = "        Order::query()->anything();";
    let diags = diagnostics(
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("app/Providers/AppServiceProvider.php", &provider),
        ],
        &consumer(body),
    )
    .await;
    assert!(diags.is_empty(), "got: {:?}", messages(&diags));
}

// ─── Missing attributes ─────────────────────────────────────────────────────

const DATABASE_CONFIG_PHP: &str = "<?php\nreturn ['default' => 'mysql'];\n";

const ORDERS_MIGRATION: &str = r#"<?php
use Illuminate\Database\Migrations\Migration;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;
return new class extends Migration {
    public function up(): void {
        Schema::create('orders', function (Blueprint $table) {
            $table->id();
            $table->string('status');
        });
    }
};
"#;

/// Unknown-member diagnostics on attribute reads, with `boot` as the
/// provider's body.
async fn attribute_diagnostics(boot: &str) -> Vec<String> {
    let provider = provider(boot);
    let body = "\
        $order->status;
        $order->lines;
        $order->statsu;";
    messages(
        &diagnostics(
            &[
                ("bootstrap/providers.php", PROVIDERS_PHP),
                ("app/Providers/AppServiceProvider.php", &provider),
                ("config/database.php", DATABASE_CONFIG_PHP),
                (
                    "database/migrations/2024_01_01_000000_create_orders.php",
                    ORDERS_MIGRATION,
                ),
            ],
            &consumer(body),
        )
        .await,
    )
}

#[tokio::test]
async fn reports_unknown_attribute_when_missing_attributes_throw() {
    assert_eq!(
        attribute_diagnostics("        Model::preventAccessingMissingAttributes();").await,
        vec!["Property App\\Models\\Order::$statsu not found"],
    );
}

#[tokio::test]
async fn strict_mode_turns_missing_attribute_reports_on() {
    assert_eq!(
        attribute_diagnostics("        Model::shouldBeStrict(! app()->isProduction());").await,
        vec!["Property App\\Models\\Order::$statsu not found"],
    );
}

#[tokio::test]
async fn unknown_attribute_is_silent_when_missing_attributes_read_null() {
    assert!(attribute_diagnostics("").await.is_empty());
    assert!(
        attribute_diagnostics("        Model::preventAccessingMissingAttributes(false);")
            .await
            .is_empty()
    );
}
