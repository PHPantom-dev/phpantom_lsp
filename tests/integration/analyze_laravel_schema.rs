//! The `analyze` command types a model's columns from the project's schema
//! dump, the way the language server does.

use crate::common::{LARAVEL_SRC_COMPOSER, create_psr4_workspace};

const DATABASE_CONFIG_PHP: &str = "\
<?php
return [
    'default' => 'mysql',
];
";

const SCHEMA_SQL: &str = "\
CREATE TABLE `orders` (
  `id` bigint NOT NULL,
  `status` varchar(255) NOT NULL,
  `created_at` timestamp NOT NULL,
  `updated_at` timestamp NULL DEFAULT NULL
);
";

const ORDER_MODEL_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Order extends Model {}
";

const CARBON_PHP: &str = "\
<?php
namespace Illuminate\\Support;
class Carbon {
    public function diffForHumans(): string { return ''; }
}
";

/// Reports member accesses on a subject whose type is unknown, so a column
/// that loses its type is caught.
const PHPANTOM_TOML: &str = "[diagnostics]\nunresolved-member-access = true\n";

/// Analyse `consumer` on its own and return the process exit code (`0`
/// clean, `1` diagnostics found).
async fn analyse_consumer(consumer: &str) -> i32 {
    let (_backend, dir) = create_psr4_workspace(
        LARAVEL_SRC_COMPOSER,
        &[
            ("config/database.php", DATABASE_CONFIG_PHP),
            ("database/schema/mysql-schema.sql", SCHEMA_SQL),
            ("src/Models/Order.php", ORDER_MODEL_PHP),
            ("stubs/Carbon.php", CARBON_PHP),
            (".phpantom.toml", PHPANTOM_TOML),
            ("src/Consumer.php", consumer),
        ],
    );

    phpantom_lsp::analyse::run(phpantom_lsp::analyse::AnalyseOptions {
        workspace_root: dir.path().to_path_buf(),
        path_filters: vec![dir.path().join("src/Consumer.php")],
        severity_filter: phpantom_lsp::analyse::SeverityFilter::All,
        use_colour: false,
        output_format: phpantom_lsp::analyse::OutputFormat::Json,
        // Suppresses the progress bar, which has no terminal to draw on here.
        debug: true,
        verbosity: 0,
        // Never the machine's own; the test asserts on the project alone.
        global_config: None,
    })
    .await
}

/// Without the schema a column is `mixed` and nothing is reported, so the
/// type error below is only found when `analyze` reads the dump.
#[tokio::test]
async fn analyze_types_columns_from_the_schema_dump() {
    let consumer = |return_type: &str, column: &str| {
        format!(
            "<?php\nnamespace App;\nuse App\\Models\\Order;\nclass Consumer {{\n    public function go(Order $order): {return_type} {{\n        return $order->{column};\n    }}\n}}\n"
        )
    };

    assert_eq!(
        analyse_consumer(&consumer("string", "status")).await,
        0,
        "a varchar column is a string"
    );
    assert_eq!(
        analyse_consumer(&consumer("array", "status")).await,
        1,
        "a varchar column returned as an array should be reported"
    );
}

/// A nullable timestamp column is the date class or `null`, and a null
/// check leaves the date class with its members.
#[tokio::test]
async fn a_nullable_timestamp_column_is_a_nullable_date() {
    let consumer = |return_type: &str, body: &str| {
        format!(
            "<?php\nnamespace App;\nuse App\\Models\\Order;\nclass Consumer {{\n    public function go(Order $order): {return_type} {{\n        {body}\n    }}\n}}\n"
        )
    };

    assert_eq!(
        analyse_consumer(&consumer(
            "string",
            "if (!is_null($order->updated_at)) { return $order->updated_at->diffForHumans(); }\n        return '';"
        ))
        .await,
        0,
        "a null-checked timestamp column is the date class"
    );
    assert_eq!(
        analyse_consumer(&consumer(
            "\\Illuminate\\Support\\Carbon",
            "return $order->created_at;"
        ))
        .await,
        0,
        "a non-null timestamp column is the date class"
    );
    assert_eq!(
        analyse_consumer(&consumer(
            "\\Illuminate\\Support\\Carbon",
            "return $order->updated_at;"
        ))
        .await,
        1,
        "a nullable timestamp column returned as non-null should be reported"
    );
}
