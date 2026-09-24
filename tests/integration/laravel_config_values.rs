//! Config keys and the values behind them.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.
//!
//! A `config('file.key')` read is resolved against the same repository
//! Laravel builds: the application's `config/*.php` files, the framework's
//! defaults merged under them the way `LoadConfiguration` merges them, and
//! the files packages register through `mergeConfigFrom()`.  What the key
//! names decides where go-to-definition lands, what completion offers,
//! whether the key is reported as unknown, and the type the read returns.

use crate::common::{
    LARAVEL_APP_COMPOSER, complete_labels_at_opened, create_initialized_psr4_workspace,
    definition_locations, goto_definition_at, markup_hover_at, messages_with_code, position_after,
    position_of,
};
use tower_lsp::lsp_types::*;

/// An application with a package installed under `vendor/acme/widgets`.
const PACKAGE_COMPOSER: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "app/", "Acme\\Widgets\\": "vendor/acme/widgets/src/" } }
}"#;

const PACKAGE_PROVIDER_LIST: &str =
    "<?php\nreturn [\n    Acme\\Widgets\\WidgetsServiceProvider::class,\n];\n";

/// A package provider merging its defaults under the `widgets` key.
const PACKAGE_CONFIG_PROVIDER: &str = "\
<?php
namespace Acme\\Widgets;

class WidgetsServiceProvider
{
    public function register(): void
    {
        $this->mergeConfigFrom(__DIR__.'/../config/widgets.php', 'widgets');
    }
}
";

const PACKAGE_CONFIG: &str = "\
<?php
return [
    'prefix' => 'w-',
    'size' => 'md',
    'theme' => [
        'color' => 'blue',
        'radius' => 4,
    ],
];
";

/// The files every package test starts from, plus `extra`.
fn package_files<'a>(extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut files = vec![
        ("bootstrap/providers.php", PACKAGE_PROVIDER_LIST),
        (
            "vendor/acme/widgets/src/WidgetsServiceProvider.php",
            PACKAGE_CONFIG_PROVIDER,
        ),
        ("vendor/acme/widgets/config/widgets.php", PACKAGE_CONFIG),
    ];
    files.extend_from_slice(extra);
    files
}

/// A class in `app/Demo.php` whose one method runs `body`.
fn demo(body: &str) -> String {
    format!(
        "<?php\nnamespace App;\nclass Demo {{\n    public function go(): void {{\n{body}\n    }}\n}}\n"
    )
}

/// The `invalid_laravel_config` messages the slow pass reports on `uri`.
fn config_diagnostics(backend: &phpantom_lsp::Backend, uri: &Url, content: &str) -> Vec<String> {
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(uri.as_str(), content, &mut diags);
    messages_with_code(&diags, "invalid_laravel_config")
}

/// The go-to-definition targets inside the first occurrence of `needle`.
async fn definitions_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    needle: &str,
) -> Vec<Location> {
    let position = position_of(content, needle);
    definition_locations(
        goto_definition_at(backend, uri, position.line, position.character + 2).await,
    )
}

/// The hover over the first `needle` (a bare `$variable;` statement).
async fn hover_on(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    content: &str,
    needle: &str,
) -> String {
    let position = position_of(content, needle);
    markup_hover_at(backend, uri, position.line, position.character + 1).await
}

// ─── Where a key is declared ────────────────────────────────────────────────

/// A config file may import classes, declare strict types, and carry
/// comments before its `return`; the returned array is still what the
/// keys are read from, and a `return` inside a comment is not.
#[tokio::test]
async fn a_config_file_with_a_preamble_is_read_from_its_return() {
    let config = "\
<?php

declare(strict_types=1);

use Illuminate\\Support\\Str;

// Application settings.
/* return ['decoy' => 1]; */
return [
    'name' => 'Acme',
    'slug' => Str::slug('Acme'),
];
";
    let consumer = demo("        config('app.slug');\n        config('app.decoy');");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/app.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    let found = definitions_at(&backend, &uri, &consumer, "app.slug").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert!(found[0].uri.as_str().ends_with("/config/app.php"));
    assert_eq!(found[0].range.start.line, 10, "'slug' is on line 10");

    let messages = config_diagnostics(&backend, &uri, &consumer);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("app.decoy"), "got {messages:?}");
}

/// A double-quoted key is a key like any other.
#[tokio::test]
async fn a_double_quoted_key_is_declared() {
    let config = "<?php\nreturn [\n    \"name\" => \"Acme\",\n    \"mail\" => [\n        \"from\" => \"noreply@example.com\",\n    ],\n];\n";
    let consumer = demo("        config('app.mail.from');");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/app.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    assert!(config_diagnostics(&backend, &uri, &consumer).is_empty());
    let found = definitions_at(&backend, &uri, &consumer, "app.mail.from").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].range.start.line, 4, "'from' is on line 4");
}

/// A key whose name contains an escaped quote is named by its unescaped
/// spelling, which is the string `config()` is called with.
#[tokio::test]
async fn a_key_with_an_escaped_quote_is_named_by_its_value() {
    let config = "<?php\nreturn [\n    'it\\'s' => 'escaped',\n];\n";
    let consumer = demo("        config(\"app.it's\");");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/app.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    let messages = config_diagnostics(&backend, &uri, &consumer);
    assert!(
        messages.is_empty(),
        "the key is `it's` once PHP reads the literal, got {messages:?}"
    );
}

// ─── Keys that do not exist ─────────────────────────────────────────────────

/// A path that runs past a scalar value, or through a nested group that
/// lacks the next segment, names nothing: `Arr::get()` hands back the
/// default for both.  The group itself is still a real key.
#[tokio::test]
async fn a_path_that_runs_past_what_the_file_declares_is_unknown() {
    let config = "\
<?php
return [
    'default' => 'mysql',
    'connections' => [
        'mysql' => [
            'host' => '127.0.0.1',
        ],
    ],
];
";
    let consumer = demo(
        "        config('database.default.deeper');\n        config('database.connections.pgsql.host');\n        config('database.connections');\n        config('database.connections.mysql.host');",
    );
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/database.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    let messages = config_diagnostics(&backend, &uri, &consumer);
    assert_eq!(messages.len(), 2, "got {messages:?}");
    assert!(
        messages
            .iter()
            .any(|m| m.contains("database.default.deeper"))
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("database.connections.pgsql.host"))
    );
}

// ─── The type a read returns ────────────────────────────────────────────────

/// A top-level key is not confused with a nested key of the same name.
#[tokio::test]
async fn a_top_level_key_is_not_confused_with_a_nested_one_of_the_same_name() {
    let config = "<?php\nreturn [\n    'components' => [\n        'prefix' => 42,\n    ],\n    'prefix' => 'acme-',\n];\n";
    let consumer = demo(
        "        $top = config('ui.prefix');\n        $top;\n        $nested = config('ui.components.prefix');\n        $nested;",
    );
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/ui.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    let top = hover_on(&backend, &uri, &consumer, "$top;").await;
    assert!(top.contains("string") && !top.contains("int"), "got {top}");
    let nested = hover_on(&backend, &uri, &consumer, "$nested;").await;
    assert!(nested.contains("int"), "got {nested}");
}

/// An empty string literal is a string.
#[tokio::test]
async fn an_empty_string_value_is_a_string() {
    let config = "<?php\nreturn [\n    'prefix' => '',\n];\n";
    let consumer = demo("        $prefix = config('ui.prefix');\n        $prefix;");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/ui.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    let hover = hover_on(&backend, &uri, &consumer, "$prefix;").await;
    assert!(hover.contains("string"), "got {hover}");
}

/// A list of class names is a list of those classes, not an empty array:
/// its entries have no string keys, but they are still there.
#[tokio::test]
async fn a_list_value_is_not_an_empty_array() {
    let config = "<?php\nreturn [\n    'handlers' => [\n        App\\Handlers\\First::class,\n        App\\Handlers\\Second::class,\n    ],\n];\n";
    let consumer = demo("        $handlers = config('pipeline.handlers');\n        $handlers;");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/pipeline.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    let hover = hover_on(&backend, &uri, &consumer, "$handlers;").await;
    assert!(
        hover.contains("list<class-string<First>|class-string<Second>>"),
        "a two-entry list should read as a list of its entries, got {hover}"
    );
}

// ─── Completion ─────────────────────────────────────────────────────────────

/// Completion offers every group and every leaf by its full dotted path,
/// but not the positions of a list.
#[tokio::test]
async fn completion_offers_groups_and_leaves_but_not_list_positions() {
    let config = "\
<?php
return [
    'default' => 'redis',
    'connections' => [
        'redis' => [
            'queue' => 'default',
        ],
    ],
    'middleware' => ['throttle', 'auth'],
];
";
    let consumer = demo("        config('');");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[("config/queue.php", config), ("app/Demo.php", &consumer)],
        "app/Demo.php",
    )
    .await;

    let position = position_after(&consumer, "config('");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    for expected in [
        "queue.default",
        "queue.connections",
        "queue.connections.redis",
        "queue.connections.redis.queue",
        "queue.middleware",
    ] {
        assert!(
            labels.iter().any(|l| l == expected),
            "missing {expected}, got {labels:?}"
        );
    }
    assert!(
        !labels.iter().any(|l| l.starts_with("queue.middleware.")),
        "list positions are not keys to offer, got {labels:?}"
    );
}

// ─── Package configuration ──────────────────────────────────────────────────

/// A package's merged defaults are keys the application can read, with the
/// types the package gives them.
#[tokio::test]
async fn a_package_config_key_is_known_and_typed() {
    let consumer = demo(
        "        $prefix = config('widgets.prefix');\n        $prefix;\n        config('widgets.nonexistent');",
    );
    let files = package_files(&[("app/Demo.php", &consumer)]);
    let (backend, _dir, uri) =
        create_initialized_psr4_workspace(PACKAGE_COMPOSER, &files, "app/Demo.php").await;

    let hover = hover_on(&backend, &uri, &consumer, "$prefix;").await;
    assert!(hover.contains("string"), "got {hover}");

    let messages = config_diagnostics(&backend, &uri, &consumer);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("widgets.nonexistent"),
        "got {messages:?}"
    );
}

/// Completion offers a package's keys alongside the application's.
#[tokio::test]
async fn completion_offers_a_packages_config_keys() {
    let consumer = demo("        config('widgets.');");
    let files = package_files(&[("app/Demo.php", &consumer)]);
    let (backend, _dir, uri) =
        create_initialized_psr4_workspace(PACKAGE_COMPOSER, &files, "app/Demo.php").await;

    let position = position_after(&consumer, "config('widgets.");
    let labels = complete_labels_at_opened(&backend, &uri, position.line, position.character).await;
    for expected in ["widgets.prefix", "widgets.theme", "widgets.theme.color"] {
        assert!(
            labels.iter().any(|l| l == expected),
            "missing {expected}, got {labels:?}"
        );
    }
}

/// `mergeConfigFrom()` merges the package's file *under* the application's:
/// a key the application publishes wins.
#[tokio::test]
async fn the_applications_published_config_wins_over_the_package_default() {
    let app_config = "<?php\nreturn [\n    'prefix' => 5,\n];\n";
    let consumer = demo(
        "        $prefix = config('widgets.prefix');\n        $prefix;\n        $size = config('widgets.size');\n        $size;",
    );
    let files = package_files(&[
        ("config/widgets.php", app_config),
        ("app/Demo.php", &consumer),
    ]);
    let (backend, _dir, uri) =
        create_initialized_psr4_workspace(PACKAGE_COMPOSER, &files, "app/Demo.php").await;

    let prefix = hover_on(&backend, &uri, &consumer, "$prefix;").await;
    assert!(
        prefix.contains("int") && !prefix.contains("string"),
        "the published value should win, got {prefix}"
    );
    // The application published only `prefix`; the package still supplies
    // the rest.
    let size = hover_on(&backend, &uri, &consumer, "$size;").await;
    assert!(size.contains("string"), "got {size}");
    assert!(config_diagnostics(&backend, &uri, &consumer).is_empty());
}

/// `mergeConfigFrom()` is a top-level `array_merge()`: a group the
/// application publishes replaces the package's group whole, so a nested
/// key only the package's copy had is gone.
#[tokio::test]
async fn a_published_group_replaces_the_packages_group_whole() {
    let app_config = "<?php\nreturn [\n    'theme' => [\n        'color' => 'red',\n    ],\n];\n";
    let consumer =
        demo("        config('widgets.theme.color');\n        config('widgets.theme.radius');");
    let files = package_files(&[
        ("config/widgets.php", app_config),
        ("app/Demo.php", &consumer),
    ]);
    let (backend, _dir, uri) =
        create_initialized_psr4_workspace(PACKAGE_COMPOSER, &files, "app/Demo.php").await;

    let messages = config_diagnostics(&backend, &uri, &consumer);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(
        messages[0].contains("widgets.theme.radius"),
        "the published `theme` has no `radius`, got {messages:?}"
    );
}

/// With nothing published, go-to-definition on a package key lands in the
/// package's own config file.
#[tokio::test]
async fn definition_of_a_package_key_reaches_the_package_file() {
    let consumer = demo("        config('widgets.size');");
    let files = package_files(&[("app/Demo.php", &consumer)]);
    let (backend, _dir, uri) =
        create_initialized_psr4_workspace(PACKAGE_COMPOSER, &files, "app/Demo.php").await;

    let found = definitions_at(&backend, &uri, &consumer, "widgets.size").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert!(
        found[0]
            .uri
            .as_str()
            .ends_with("/vendor/acme/widgets/config/widgets.php"),
        "got {found:?}"
    );
    assert_eq!(found[0].range.start.line, 3, "'size' is on line 3");
}

/// When the application publishes only part of a package's config, a key
/// it left out is still declared in the package's file, and that is where
/// go-to-definition belongs.
#[tokio::test]
async fn definition_of_an_unpublished_package_key_reaches_the_package_file() {
    let app_config = "<?php\nreturn [\n    'prefix' => 'app-',\n];\n";
    let consumer = demo("        config('widgets.size');");
    let files = package_files(&[
        ("config/widgets.php", app_config),
        ("app/Demo.php", &consumer),
    ]);
    let (backend, _dir, uri) =
        create_initialized_psr4_workspace(PACKAGE_COMPOSER, &files, "app/Demo.php").await;

    let found = definitions_at(&backend, &uri, &consumer, "widgets.size").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert!(
        found[0]
            .uri
            .as_str()
            .ends_with("/vendor/acme/widgets/config/widgets.php"),
        "the key lives in the package's file, got {found:?}"
    );
    assert_eq!(found[0].range.start.line, 3, "'size' is on line 3");
}

// ─── Framework defaults ─────────────────────────────────────────────────────

const FRAMEWORK_APP_CONFIG: &str = "\
<?php
return [
    'name' => env('APP_NAME', 'Laravel'),
    'faker_locale' => 'en_US',
];
";

const FRAMEWORK_DATABASE_CONFIG: &str = "\
<?php
return [
    'default' => env('DB_CONNECTION', 'sqlite'),
    'connections' => [
        'sqlite' => [
            'driver' => 'sqlite',
        ],
        'mysql' => [
            'driver' => 'mysql',
            'host' => '127.0.0.1',
        ],
    ],
    'redis' => [
        'client' => 'phpredis',
        'options' => [
            'cluster' => 'redis',
        ],
    ],
];
";

/// The application's `config/database.php` overrides one connection and
/// the whole `redis` group.
const APP_DATABASE_CONFIG: &str = "\
<?php
return [
    'connections' => [
        'mysql' => [
            'driver' => 'mysql',
            'port' => 3306,
        ],
    ],
    'redis' => [
        'client' => 'predis',
    ],
];
";

/// A key the application's `config/app.php` leaves out still exists: the
/// framework's own copy is merged beneath it.
#[tokio::test]
async fn a_framework_default_key_the_application_omits_is_known() {
    let consumer = demo(
        "        config('app.faker_locale');\n        config('app.fakerlocale');\n        config('');",
    );
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            (
                "config/app.php",
                "<?php\nreturn [\n    'name' => 'Acme',\n];\n",
            ),
            (
                "vendor/laravel/framework/config/app.php",
                FRAMEWORK_APP_CONFIG,
            ),
            ("app/Demo.php", &consumer),
        ],
        "app/Demo.php",
    )
    .await;

    let messages = config_diagnostics(&backend, &uri, &consumer);
    assert_eq!(messages.len(), 1, "got {messages:?}");
    assert!(messages[0].contains("app.fakerlocale"), "got {messages:?}");

    // Inside the quotes of the empty `config('')` call.
    let position = position_of(&consumer, "config('')");
    let labels = complete_labels_at_opened(
        &backend,
        &uri,
        position.line,
        position.character + "config('".len() as u32,
    )
    .await;
    assert!(
        labels.iter().any(|l| l == "app.faker_locale"),
        "got {labels:?}"
    );
}

/// Go-to-definition on a framework default the application does not
/// override lands on the framework's declaration of it.
#[tokio::test]
async fn definition_of_a_framework_default_key_reaches_the_framework_file() {
    let consumer = demo("        config('app.faker_locale');");
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            (
                "config/app.php",
                "<?php\nreturn [\n    'name' => 'Acme',\n];\n",
            ),
            (
                "vendor/laravel/framework/config/app.php",
                FRAMEWORK_APP_CONFIG,
            ),
            ("app/Demo.php", &consumer),
        ],
        "app/Demo.php",
    )
    .await;

    let found = definitions_at(&backend, &uri, &consumer, "app.faker_locale").await;
    assert_eq!(found.len(), 1, "got {found:?}");
    assert!(
        found[0]
            .uri
            .as_str()
            .ends_with("/vendor/laravel/framework/config/app.php"),
        "the key is only declared by the framework, got {found:?}"
    );
    assert_eq!(found[0].range.start.line, 3, "'faker_locale' is on line 3");
}

/// `connections` is one of the options `LoadConfiguration` merges entry by
/// entry, so a framework connection the application does not mention is
/// still configured.
#[tokio::test]
async fn a_mergeable_framework_option_keeps_the_entries_the_application_omits() {
    let consumer = demo(
        "        $driver = config('database.connections.sqlite.driver');\n        $driver;\n        config('database.connections.mysql.port');\n        config('database.default');",
    );
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("config/database.php", APP_DATABASE_CONFIG),
            (
                "vendor/laravel/framework/config/database.php",
                FRAMEWORK_DATABASE_CONFIG,
            ),
            ("app/Demo.php", &consumer),
        ],
        "app/Demo.php",
    )
    .await;

    assert!(
        config_diagnostics(&backend, &uri, &consumer).is_empty(),
        "every key read here is configured"
    );
    let driver = hover_on(&backend, &uri, &consumer, "$driver;").await;
    assert!(driver.contains("string"), "got {driver}");
}

/// Below the mergeable options, the application's value replaces the
/// framework's whole: its `mysql` connection has no `host`, and its
/// `redis` group has no `options`.
#[tokio::test]
async fn an_application_group_replaces_the_framework_group_whole() {
    let consumer = demo(
        "        config('database.connections.mysql.host');\n        config('database.redis.options');\n        $redis = config('database.redis');\n        $redis;",
    );
    let (backend, _dir, uri) = create_initialized_psr4_workspace(
        LARAVEL_APP_COMPOSER,
        &[
            ("config/database.php", APP_DATABASE_CONFIG),
            (
                "vendor/laravel/framework/config/database.php",
                FRAMEWORK_DATABASE_CONFIG,
            ),
            ("app/Demo.php", &consumer),
        ],
        "app/Demo.php",
    )
    .await;

    let messages = config_diagnostics(&backend, &uri, &consumer);
    assert_eq!(messages.len(), 2, "got {messages:?}");
    assert!(
        messages
            .iter()
            .any(|m| m.contains("database.connections.mysql.host"))
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("database.redis.options"))
    );

    let redis = hover_on(&backend, &uri, &consumer, "$redis;").await;
    assert!(
        redis.contains("client") && !redis.contains("options"),
        "the application's `redis` group is the whole value, got {redis}"
    );
}
