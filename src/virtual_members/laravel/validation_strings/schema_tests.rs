use super::*;

fn migration(content: &str, table: &str, column: Option<&str>) -> Option<usize> {
    migration_definition(content, table, column, "mysql", "mysql")
}

#[test]
fn validation_schema_navigation_rejects_empty_table_names() {
    let backend = Backend::new_test();
    assert!(definition(&backend, "", None).is_none());
    backend
        .laravel
        .schema_index
        .write()
        .connection_drivers
        .insert("mysql".into(), "mysql".into());
    assert!(definition(&backend, "mysql.", Some("email")).is_none());
    assert!(definition(&backend, "missing", Some("email")).is_none());
    assert!(definition(&backend, "unknown.users", Some("email")).is_none());
}

#[test]
fn validation_schema_navigation_skips_later_migrations_without_the_declaration() {
    let root = tempfile::tempdir().unwrap();
    let migrations = root.path().join("database/migrations");
    std::fs::create_dir_all(&migrations).unwrap();
    std::fs::write(
        migrations.join("2024_01_01_000000_create_users.php"),
        r#"<?php
use Illuminate\Support\Facades\Schema;
Schema::create('users', function ($table) { $table->id(); });
"#,
    )
    .unwrap();
    std::fs::write(
        migrations.join("2024_01_02_000000_index_users.php"),
        r#"<?php
use Illuminate\Support\Facades\Schema;
Schema::table('users', function ($table) { $table->index('id'); });
"#,
    )
    .unwrap();
    let backend = Backend::new_test();
    *backend.laravel.schema_index.write() =
        super::super::super::database_schema::load_schema_index(
            root.path(),
            &crate::config::LaravelConfig::default(),
            &std::collections::HashMap::new(),
        )
        .unwrap();
    for column in [None, Some("id")] {
        let location = definition(&backend, "users", column).unwrap();
        assert!(
            location
                .uri
                .as_str()
                .ends_with("/2024_01_01_000000_create_users.php")
        );
    }
}

#[test]
fn validation_schema_navigation_uses_the_table_blueprint_and_up_method() {
    let source = r#"<?php
use Illuminate\Support\Facades\Schema;
return new class {
    public function up() {
        Schema::create('users', function ($table) {
            $table->string('email')->nullable();
            $table->unique('email');
            $other->string('email');
            $callback = function ($table) { $table->string('email'); };
        });
        Schema::create('orders', function ($table) { $table->string('email'); });
    }
    public function down() {
        Schema::table('users', function ($table) { $table->string('email'); });
    }
};
"#;
    assert_eq!(
        migration(source, "users", None),
        Some(source.find("'users'").unwrap() + 1)
    );
    assert_eq!(
        migration(source, "users", Some("email")),
        Some(source.find("'email'").unwrap() + 1)
    );
    assert!(migration(source, "users", Some("missing")).is_none());
    assert!(migration(source, "missing", None).is_none());
}

#[test]
fn validation_schema_navigation_ignores_uninitialized_properties() {
    let source = r#"<?php
use Illuminate\Support\Facades\Schema;
return new class {
    protected string $other;
    protected $connection = 'mysql';
    public function up() {
        Schema::create('users', function ($table) { $table->id(); });
    }
};
"#;
    assert_eq!(
        migration(source, "users", None),
        Some(source.find("'users'").unwrap() + 1)
    );
}

#[test]
fn validation_schema_navigation_resolves_grouped_imports_and_rejects_namesakes() {
    let source = r#"<?php
namespace App {
    use Illuminate\Support\Facades\{Schema as Tables, Route};
    class Migration {
        public function up() {
            Tables::create(callback: function ($blueprint) {
                $blueprint->string(column: 'email');
            }, table: 'users');
            Schema::create('users', function ($table) { $table->string('email'); });
        }
    }
}
namespace Other {
    use App\Schema;
    class AnotherMigration {
        public function up() {
            Schema::create('users', function ($table) { $table->string('email'); });
        }
    }
}
"#;
    assert_eq!(
        migration(source, "users", Some("email")),
        Some(source.find("'email'").unwrap() + 1)
    );
    assert_eq!(
        migration(source, "users", None),
        Some(source.find("table: 'users'").unwrap() + 8)
    );
}

#[test]
fn validation_schema_navigation_honors_connection_selection() {
    let source = r#"<?php
use Illuminate\Support\Facades\Schema;
class Migration {
    protected $connection = 'archive';
    public function up() {
        Schema::create('users', function ($table) { $table->string('email'); });
        Schema::connection('mysql')->create('users', function ($table) { $table->string('email'); });
        Schema::connection($dynamic)->create('users', function ($table) { $table->string('email'); });
    }
}
"#;
    let first = source.find("'email'").unwrap() + 1;
    let second = source[first..].find("'email'").unwrap() + first + 1;
    assert_eq!(
        migration_definition(source, "users", Some("email"), "archive", "mysql"),
        Some(first)
    );
    assert_eq!(migration(source, "users", Some("email")), Some(second));
    let source = "<?php use Illuminate\\Support\\Facades\\Schema; Schema::create('users', function ($table) { $table->string('email'); });";
    assert!(migration_definition(source, "users", None, "archive", "mysql").is_none());
}

#[test]
fn validation_schema_navigation_handles_implicit_columns_and_alterations() {
    let source = r#"<?php
use Illuminate\Support\Facades\Schema;
Schema::table('users', function ($table) {
    $table->id();
    $table->timestamps();
    $table->softDeletes();
    $table->rememberToken();
    $table->morphs('owner');
    $table->addColumn('string', 'nickname');
    $table->renameColumn('old_name', 'new_name');
    $table->after('id', function ($extra) { $extra->string('email'); });
    $table->dropColumn('email');
});
"#;
    for (column, site) in [
        ("id", "id()"),
        ("created_at", "timestamps()"),
        ("updated_at", "timestamps()"),
        ("deleted_at", "softDeletes()"),
        ("remember_token", "rememberToken()"),
        ("owner_id", "owner'"),
        ("owner_type", "owner'"),
        ("nickname", "nickname'"),
        ("new_name", "new_name'"),
        ("email", "email'"),
    ] {
        assert_eq!(
            migration(source, "users", Some(column)),
            source.find(site),
            "{column}"
        );
    }
    assert!(migration(source, "users", None).is_none());
    assert!(migration(source, "users", Some("other_id")).is_none());
}

#[test]
fn validation_schema_navigation_ignores_unresolved_and_nonliteral_calls() {
    let source = r#"<?php
use Illuminate\Support\Facades\Schema;
Schema::$method('users');
Schema::drop('users');
Schema::create($name, function ($table) { $table->string('email'); });
Schema::table('users', $callback);
Schema::table('users', function () {});
Schema::table('users', function ($table) {
    $table->$method('email');
    $table->string($name);
    $table->morphs($name);
    $table->uuid();
    $table->ulid();
    $table->ipAddress();
    $table->macAddress();
    $table->after('id', $callback);
});
"#;
    assert!(migration(source, "users", Some("email")).is_none());
    for (column, method) in [
        ("uuid", "uuid()"),
        ("ulid", "ulid()"),
        ("ip_address", "ipAddress()"),
        ("mac_address", "macAddress()"),
    ] {
        assert_eq!(
            migration(source, "users", Some(column)),
            source.find(method)
        );
    }
}

#[test]
fn validation_schema_sql_navigation_uses_identifier_tokens() {
    let source = "-- CREATE TABLE fake (email text);\nCREATE TABLE IF NOT EXISTS `users` (\n  `id` BIGINT,\n  `email` VARCHAR(255),\n  `other` TEXT DEFAULT 'email',\n  CONSTRAINT email_unique UNIQUE (`email`)\n);\nCREATE TABLE orders (email text);\n";
    assert_eq!(
        sql_definition(source, "users", None),
        Some(Position::new(1, 27))
    );
    assert_eq!(
        sql_definition(source, "users", Some("email")),
        Some(Position::new(3, 2))
    );
    assert!(sql_definition(source, "users", Some("missing")).is_none());
    assert!(sql_definition(source, "fake", None).is_none());
    assert!(sql_definition("CREATE TABLE users", "users", Some("id")).is_none());
    assert!(sql_definition("CREATE TABLE users ('unterminated", "users", None).is_none());
}

#[test]
fn validation_schema_sql_navigation_handles_qualification_and_unicode() {
    let source = "/* 🙂 */ CREATE TABLE public.\"users\" (\"é_mail\" text, price decimal(10, 2));";
    let table = source.find("\"users\"").unwrap();
    let column = source.find("\"é_mail\"").unwrap();
    assert_eq!(
        sql_definition(source, "users", None),
        Some(crate::text_position::offset_to_position(source, table))
    );
    assert_eq!(
        sql_definition(source, "users", Some("é_mail")),
        Some(crate::text_position::offset_to_position(source, column))
    );
    assert!(sql_definition(source, "users", Some("absent")).is_none());
}

#[test]
fn validation_schema_sources_reuse_the_discovery_plan() {
    use crate::virtual_members::laravel::database_schema::load_schema_index;

    let dir = tempfile::tempdir().unwrap();
    let migrations = dir.path().join("database/migrations");
    let dumps = dir.path().join("database/schema");
    std::fs::create_dir_all(&migrations).unwrap();
    std::fs::create_dir_all(&dumps).unwrap();
    let migration = migrations.join("2026_01_01_users.php");
    std::fs::write(
        &migration,
        "<?php return new class { public function up() {} };",
    )
    .unwrap();
    let mysql = dumps.join("mysql-schema.sql");
    std::fs::write(&mysql, "CREATE TABLE users (id bigint);").unwrap();
    std::fs::write(
        dumps.join("archive-schema.sql"),
        "CREATE TABLE archived (id bigint);",
    )
    .unwrap();
    let index = load_schema_index(
        dir.path(),
        &crate::config::LaravelConfig::default(),
        &std::collections::HashMap::new(),
    )
    .unwrap();
    assert_eq!(index.migration_sources().next().unwrap().0, migration);
    assert_eq!(
        index.dump_paths_for("mysql").collect::<Vec<_>>(),
        [mysql.as_path()]
    );
    assert!(index.dump_paths_for("missing").next().is_none());
}
