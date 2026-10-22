use super::*;
use crate::config::{LaravelMigrationsConfig, LaravelSchemaConfig};

fn write_migration(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "<?php\n").unwrap();
    path
}

/// Editing a migration must never add a table the initial scan would
/// not have found, or the schema index depends on which files happened
/// to be touched during the session.
#[test]
fn watched_migrations_agree_with_the_default_scan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let visible = write_migration(root, "database/migrations/2024_01_01_000000_users.php");
    let ignored = write_migration(
        root,
        "build/database/migrations/2024_01_01_000001_posts.php",
    );
    let hidden = write_migration(
        root,
        ".cache/database/migrations/2024_01_01_000002_tags.php",
    );
    let vendored = write_migration(
        root,
        "vendor/acme/pkg/database/migrations/2024_01_01_000003_logs.php",
    );
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::write(root.join(".gitignore"), "/build/\n").unwrap();

    let mut scanned = Vec::new();
    collect_default_migration_files(root, &mut scanned).unwrap();
    assert_eq!(scanned, vec![visible.clone()]);

    let config = LaravelMigrationsConfig::default();
    let mut discovery = MigrationDiscovery::default();
    for path in [&visible, &ignored, &hidden, &vendored] {
        assert_eq!(
            discovery.is_discoverable(root, &config, path),
            scanned.contains(path),
            "{} disagrees with the default scan",
            path.display()
        );
    }
}

/// Explicit paths are read with a plain `read_dir` on both sides, so
/// naming an ignored directory still opts it in.
#[test]
fn configured_migration_paths_ignore_the_walk_rules() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let ignored = write_migration(
        root,
        "build/database/migrations/2024_01_01_000000_posts.php",
    );
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::write(root.join(".gitignore"), "/build/\n").unwrap();

    let config = LaravelMigrationsConfig {
        enabled: None,
        paths: vec!["build/database/migrations".to_string()],
    };
    let scanned = discover_migration_files(root, &config).unwrap();
    assert_eq!(scanned, vec![ignored.clone()]);
    assert!(MigrationDiscovery::default().is_discoverable(root, &config, &ignored));
}

#[test]
fn database_type_to_php_type_maps_arrays_before_scalar_elements() {
    // Postgres array types must resolve to `array`, not the scalar
    // element type embedded in the name.
    assert_eq!(
        database_type_to_php_type("integer[]", false),
        PhpType::array()
    );
    assert_eq!(
        database_type_to_php_type("bigint[]", false),
        PhpType::array()
    );
    assert_eq!(
        database_type_to_php_type("numeric[]", false),
        PhpType::array()
    );
    assert_eq!(
        database_type_to_php_type("boolean[]", false),
        PhpType::array()
    );
    assert_eq!(database_type_to_php_type("text[]", false), PhpType::array());
}

#[test]
fn database_type_to_php_type_does_not_treat_interval_as_int() {
    // `interval` embeds "int" but is a string type.
    assert_eq!(
        database_type_to_php_type("interval", false),
        PhpType::string()
    );
}

#[test]
fn database_type_to_php_type_treats_tinyint_one_as_bool() {
    // `$table->boolean()` renders as `tinyint(1)` in a MySQL dump.
    assert_eq!(
        database_type_to_php_type("tinyint(1)", false),
        PhpType::bool()
    );
    // Wider tinyints stay integers.
    assert_eq!(
        database_type_to_php_type("tinyint(3)", false),
        PhpType::int()
    );
    assert_eq!(database_type_to_php_type("tinyint", false), PhpType::int());
    // Plain integer types are unaffected.
    assert_eq!(database_type_to_php_type("bigint", false), PhpType::int());
}

#[test]
fn parses_postgres_create_table() {
    let sql = r#"
        CREATE TABLE public.users (
            id bigserial PRIMARY KEY,
            email character varying(255) NOT NULL,
            settings jsonb,
            active boolean DEFAULT false NOT NULL,
            CONSTRAINT users_email_unique UNIQUE (email)
        );
    "#;
    let tables = parse_schema_dump("pgsql", sql);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].name, "users");
    assert_eq!(tables[0].columns[0].name, "id");
    assert_eq!(tables[0].columns[1].name, "email");
    assert_eq!(
        tables[0].columns[1].database_type.to_ascii_lowercase(),
        "character varying(255)"
    );
    assert!(!tables[0].columns[1].nullable);
    assert_eq!(
        tables[0].columns[2].php_type,
        PhpType::union(vec![PhpType::array(), PhpType::null()]).simplified()
    );
}

#[test]
fn parses_postgres_dump_with_restrict_and_generated_column() {
    let sql = r#"
        -- PostgreSQL database dump
        \restrict abc123

        CREATE TABLE public.users (
            id integer NOT NULL,
            first_name text NOT NULL,
            last_name text NOT NULL,
            name character varying(255) GENERATED ALWAYS AS (((first_name || ' '::text) || last_name)) NOT NULL
        );

        \unrestrict abc123
    "#;

    let tables = parse_schema_dump("pgsql", sql);
    assert_eq!(tables.len(), 1);
    let name = tables[0].column("name").expect("generated column");
    assert_eq!(
        name.database_type.to_ascii_lowercase(),
        "character varying(255)"
    );
    assert!(!name.nullable);
    assert_eq!(name.generated_mode.as_deref(), Some("virtual"));
    assert_eq!(
        name.generated_expression.as_deref(),
        Some("((first_name || ' '::TEXT) || last_name)")
    );
}

#[test]
fn keeps_connections_separate() {
    let mut index = SchemaIndex::default();
    for table in parse_schema_dump("mysql", "CREATE TABLE users (id int NOT NULL);") {
        index.insert_table(table);
    }
    for table in parse_schema_dump("analytics", "CREATE TABLE users (uuid varchar(36));") {
        index.insert_table(table);
    }
    assert!(index.column_source("mysql", "users", "id").is_some());
    assert!(index.column_source("analytics", "users", "uuid").is_some());
    assert!(index.column_source("mysql", "users", "uuid").is_none());
}

#[test]
fn parses_final_statement_without_trailing_semicolon() {
    let tables = parse_schema_dump(
        "primary",
        "CREATE TABLE users (id bigint NOT NULL, email varchar(255))",
    );
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].name, "users");
    assert_eq!(tables[0].columns.len(), 2);
}

#[test]
fn loads_laravel_schema_directory_and_database_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::create_dir_all(dir.path().join("database/schema")).unwrap();
    std::fs::write(
        dir.path().join("config/database.php"),
        r#"<?php return [
            'default' => env('DB_CONNECTION', 'primary'),
            'connections' => [
                'primary' => ['driver' => 'pgsql'],
                'analytics' => ['driver' => 'mysql'],
            ],
        ];"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("database/schema/primary-schema.sql"),
        "CREATE TABLE users (id bigint NOT NULL, email varchar(255));",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("database/schema/analytics-schema.sql"),
        "CREATE TABLE users (uuid varchar(36) NOT NULL);",
    )
    .unwrap();

    let index = load_schema_index(dir.path(), &LaravelConfig::default(), &HashMap::new()).unwrap();
    assert_eq!(index.default_connection.as_deref(), Some("primary"));
    assert_eq!(
        index.connection_drivers.get("primary").map(String::as_str),
        Some("pgsql")
    );
    assert!(index.column_source("primary", "users", "email").is_some());
    assert!(index.column_source("analytics", "users", "uuid").is_some());
}

#[test]
fn watched_paths_detect_schema_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let default_config = LaravelConfig::default();

    assert!(SchemaIndex::watched_path_affects_schema(
        root,
        &default_config,
        &root.join("database/schema/primary-schema.sql"),
    ));
    assert!(SchemaIndex::watched_path_affects_schema(
        root,
        &default_config,
        &root.join("config/database.php"),
    ));
    assert!(SchemaIndex::watched_path_affects_schema(
        root,
        &default_config,
        &root.join(".phpantom.toml"),
    ));
    assert!(!SchemaIndex::watched_path_affects_schema(
        root,
        &default_config,
        &root.join("storage/debug.sql"),
    ));

    let custom_config = LaravelConfig {
        schema: LaravelSchemaConfig {
            paths: vec!["extra/schema.sql".to_string(), "tenant/schema".to_string()],
            ..LaravelSchemaConfig::default()
        },
        ..LaravelConfig::default()
    };
    assert!(SchemaIndex::watched_path_affects_schema(
        root,
        &custom_config,
        &root.join("extra/schema.sql"),
    ));
    assert!(SchemaIndex::watched_path_affects_schema(
        root,
        &custom_config,
        &root.join("tenant/schema/eagle-schema.sql"),
    ));
    assert!(!SchemaIndex::watched_path_affects_schema(
        root,
        &custom_config,
        &root.join("database/schema/primary-schema.sql"),
    ));
}

#[test]
fn applies_laravel_migrations_over_schema_dump() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("database/schema")).unwrap();
    std::fs::create_dir_all(root.join("database/migrations")).unwrap();
    std::fs::write(
        root.join("database/schema/default-schema.sql"),
        "CREATE TABLE users (id bigint NOT NULL, old_name varchar(255), removed_at timestamp);",
    )
    .unwrap();
    std::fs::write(
        root.join("database/migrations/2024_01_01_000000_update_users.php"),
        r#"<?php
        return new class {
            public function up(): void
            {
                Schema::table('users', function (Blueprint $table): void {
                    $table->string('email')->nullable(false);
                    $table->renameColumn('old_name', 'name');
                    $table->dropColumn('removed_at');
                });
            }
        };
        "#,
    )
    .unwrap();

    let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
    assert!(index.column_source("default", "users", "id").is_some());
    assert!(index.column_source("default", "users", "email").is_some());
    assert!(index.column_source("default", "users", "name").is_some());
    assert!(
        index
            .column_source("default", "users", "old_name")
            .is_none()
    );
    assert!(
        index
            .column_source("default", "users", "removed_at")
            .is_none()
    );
}

#[test]
fn discovers_direct_non_vendor_migrations_only() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("database/migrations/archive")).unwrap();
    std::fs::create_dir_all(root.join("modules/blog/database/migrations")).unwrap();
    std::fs::create_dir_all(root.join("vendor/package/database/migrations")).unwrap();
    std::fs::write(
        root.join("database/migrations/2024_01_01_000000_create_posts.php"),
        r#"<?php
        return new class {
            public function up(): void
            {
                Schema::create('posts', function (Blueprint $table): void {
                    $table->id();
                    $table->string('title');
                });
            }
        };
        "#,
    )
    .unwrap();
    std::fs::write(
        root.join("database/migrations/archive/2024_01_02_000000_create_archived.php"),
        r#"<?php return new class { public function up(): void { Schema::create('archived_posts', function (Blueprint $table): void { $table->id(); }); } };"#,
    )
    .unwrap();
    std::fs::write(
        root.join("modules/blog/database/migrations/2024_01_03_000000_create_comments.php"),
        r#"<?php return new class { public function up(): void { Schema::create('comments', function (Blueprint $table): void { $table->id(); }); } };"#,
    )
    .unwrap();
    std::fs::write(
        root.join("vendor/package/database/migrations/2024_01_04_000000_create_vendor.php"),
        r#"<?php return new class { public function up(): void { Schema::create('vendor_rows', function (Blueprint $table): void { $table->id(); }); } };"#,
    )
    .unwrap();

    let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
    assert!(index.column_source("default", "posts", "title").is_some());
    assert!(index.column_source("default", "comments", "id").is_some());
    assert!(
        index
            .column_source("default", "archived_posts", "id")
            .is_none()
    );
    assert!(
        index
            .column_source("default", "vendor_rows", "id")
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn default_migration_discovery_skips_ignored_and_symlinked_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let migration = root.join("database/migrations/2024_01_01_000000_create_posts.php");
    std::fs::create_dir_all(migration.parent().unwrap()).unwrap();
    std::fs::write(&migration, "<?php").unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::write(root.join(".gitignore"), "/ignored/\n").unwrap();
    std::fs::create_dir_all(root.join("ignored/database/migrations")).unwrap();
    std::fs::write(
        root.join("ignored/database/migrations/2024_01_02_000000_ignored.php"),
        "<?php",
    )
    .unwrap();
    std::os::unix::fs::symlink(root, root.join("workspace-loop")).unwrap();

    let files = discover_migration_files(root, &LaravelMigrationsConfig::default()).unwrap();

    assert_eq!(files, vec![migration]);
}

#[test]
fn applies_migrations_by_basename_across_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("database/migrations")).unwrap();
    std::fs::create_dir_all(root.join("modules/blog/database/migrations")).unwrap();
    std::fs::write(
        root.join("modules/blog/database/migrations/2024_01_01_000000_create_users.php"),
        r#"<?php
        return new class {
            public function up(): void
            {
                Schema::create('users', function (Blueprint $table): void {
                    $table->id();
                    $table->string('name');
                });
            }
        };
        "#,
    )
    .unwrap();
    std::fs::write(
        root.join("database/migrations/2024_02_01_000000_drop_name.php"),
        r#"<?php
        return new class {
            public function up(): void
            {
                Schema::table('users', function (Blueprint $table): void {
                    $table->dropColumn('name');
                });
            }
        };
        "#,
    )
    .unwrap();

    let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
    assert!(index.column_source("default", "users", "id").is_some());
    assert!(index.column_source("default", "users", "name").is_none());
}

#[test]
fn applies_migration_table_drop_rename_and_connections() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("database/migrations")).unwrap();
    std::fs::write(
        root.join("database/migrations/2024_01_01_000000_connections.php"),
        r#"<?php
        return new class {
            protected $connection = 'tenant';

            public function up(): void
            {
                Schema::create('drafts', function (Blueprint $table): void {
                    $table->id();
                    $table->boolean('published')->nullable();
                });
                Schema::rename('drafts', 'articles');
                Schema::connection('analytics')->create('events', function (Blueprint $table): void {
                    $table->uuid('uuid');
                });
                Schema::connection('analytics')->dropIfExists('temporary_events');
            }
        };
        "#,
    )
    .unwrap();

    let index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
    assert!(index.column_source("tenant", "drafts", "id").is_none());
    assert!(
        index
            .column_source("tenant", "articles", "published")
            .is_some()
    );
    assert!(index.column_source("analytics", "events", "uuid").is_some());
    assert!(index.column_source("tenant", "events", "uuid").is_none());
}

#[test]
fn loads_checked_in_laravel_migration_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laravel_migrations");
    let index = load_schema_index(&root, &LaravelConfig::default(), &HashMap::new()).unwrap();

    assert_schema_column(&index, "default", "users", "id", "BIGINT", false, None);
    assert_schema_column(&index, "default", "users", "name", "VARCHAR", false, None);
    assert_schema_column(&index, "default", "users", "email", "VARCHAR", true, None);
    assert!(
        index
            .column_source("default", "users", "address1")
            .is_none()
    );
    assert_schema_column(
        &index,
        "default",
        "users",
        "created_at",
        "TIMESTAMP",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "default",
        "users",
        "updated_at",
        "TIMESTAMP",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "default",
        "users",
        "deleted_at",
        "TIMESTAMP",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "default",
        "users",
        "ip_address",
        "VARCHAR",
        false,
        None,
    );
    assert_schema_column(
        &index,
        "default",
        "users",
        "custom_mac_address",
        "VARCHAR",
        false,
        None,
    );
    assert_schema_column(&index, "default", "users", "uuid", "UUID", false, None);
    assert_schema_column(
        &index,
        "default",
        "users",
        "custom_ulid",
        "CHAR(26)",
        false,
        None,
    );

    let display_name = index
        .column_source("default", "users", "display_name")
        .expect("users.display_name should be generated");
    assert_eq!(display_name.generated_mode.as_deref(), Some("virtual"));
    assert_eq!(
        display_name.generated_expression.as_deref(),
        Some("concat(name, ' <', email, '>')")
    );

    let email_hash = index
        .column_source("default", "users", "email_hash")
        .expect("users.email_hash should be generated");
    assert_eq!(email_hash.generated_mode.as_deref(), Some("stored"));
    assert_eq!(
        email_hash.generated_expression.as_deref(),
        Some("md5(email)")
    );

    assert_schema_column(
        &index, "billing", "invoices", "total", "DECIMAL", false, None,
    );
    assert_schema_column(
        &index, "billing", "invoices", "metadata", "JSON", true, None,
    );
    assert_schema_column(
        &index,
        "analytics",
        "events",
        "occurred_at",
        "TIMESTAMP",
        true,
        None,
    );
}

#[test]
fn incremental_migration_update_replays_from_base() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("database/schema")).unwrap();
    std::fs::create_dir_all(root.join("database/migrations")).unwrap();
    std::fs::write(
        root.join("database/schema/default-schema.sql"),
        "CREATE TABLE users (id bigint NOT NULL, email varchar(255));",
    )
    .unwrap();
    std::fs::write(
        root.join("database/migrations/2024_01_01_000000_add_name.php"),
        r#"<?php return new class { public function up(): void { Schema::table('users', function (Blueprint $table): void { $table->string('name'); }); } };"#,
    )
    .unwrap();

    let mut index = load_schema_index(root, &LaravelConfig::default(), &HashMap::new()).unwrap();
    assert!(index.column_source("default", "users", "id").is_some());
    assert!(index.column_source("default", "users", "email").is_some());
    assert!(index.column_source("default", "users", "name").is_some());
    assert!(index.column_source("default", "users", "age").is_none());

    index.update_migration_file(
        &root.join("database/migrations/2024_01_01_000000_add_name.php"),
        r#"<?php return new class { public function up(): void { Schema::table('users', function (Blueprint $table): void { $table->string('name'); $table->integer('age'); }); } };"#.to_string(),
    );
    assert!(index.column_source("default", "users", "name").is_some());
    assert!(index.column_source("default", "users", "age").is_some());
    assert!(index.column_source("default", "users", "email").is_some());

    let new_migration = root.join("database/migrations/2024_02_01_000000_drop_email.php");
    index.update_migration_file(
        &new_migration,
        r#"<?php return new class { public function up(): void { Schema::table('users', function (Blueprint $table): void { $table->dropColumn('email'); }); } };"#.to_string(),
    );
    assert!(index.column_source("default", "users", "email").is_none());
    assert!(index.column_source("default", "users", "name").is_some());

    index.remove_migration_file(&new_migration);
    assert!(index.column_source("default", "users", "email").is_some());
}

#[test]
fn expands_blueprint_macros_during_migration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("database/migrations")).unwrap();
    std::fs::write(
        root.join("database/migrations/2024_01_01_000000_create_orders.php"),
        r#"<?php
        return new class {
            public function up(): void
            {
                Schema::create('orders', function (Blueprint $table): void {
                    $table->id();
                    $table->auditColumns();
                    $table->string('status');
                });
            }
        };
        "#,
    )
    .unwrap();

    let mut macros = HashMap::new();
    macros.insert(
        "auditColumns".to_string(),
        r#"function () {
            $this->unsignedBigInteger('created_by')->nullable();
            $this->unsignedBigInteger('updated_by')->nullable();
            $this->timestamps();
        }"#
        .to_string(),
    );

    let index = load_schema_index(root, &LaravelConfig::default(), &macros).unwrap();
    assert_schema_column(&index, "default", "orders", "id", "BIGINT", false, None);
    assert_schema_column(
        &index, "default", "orders", "status", "VARCHAR", false, None,
    );
    assert_schema_column(
        &index,
        "default",
        "orders",
        "created_by",
        "BIGINT",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "default",
        "orders",
        "updated_by",
        "BIGINT",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "default",
        "orders",
        "created_at",
        "TIMESTAMP",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "default",
        "orders",
        "updated_at",
        "TIMESTAMP",
        true,
        None,
    );
}

#[test]
fn loads_checked_in_schema_dump_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/schema_dumps");
    let index = load_schema_index(&root, &LaravelConfig::default(), &HashMap::new()).unwrap();

    assert_eq!(index.default_connection.as_deref(), Some("primary"));
    assert_eq!(
        index.connection_drivers.get("primary").map(String::as_str),
        Some("pgsql")
    );
    assert_eq!(
        index
            .connection_drivers
            .get("analytics")
            .map(String::as_str),
        Some("mysql")
    );
    assert_eq!(
        index.connection_drivers.get("archive").map(String::as_str),
        Some("sqlite")
    );
    assert_eq!(
        index
            .connection_drivers
            .get("pgsql_types")
            .map(String::as_str),
        Some("pgsql")
    );
    assert_eq!(
        index
            .connection_drivers
            .get("mysql_types")
            .map(String::as_str),
        Some("mysql")
    );
    assert_eq!(
        index
            .connection_drivers
            .get("sqlite_types")
            .map(String::as_str),
        Some("sqlite")
    );

    let primary_email = index
        .column_source("primary", "users", "email")
        .expect("primary users.email should be loaded");
    assert_eq!(
        primary_email.database_type.to_ascii_lowercase(),
        "character varying(255)"
    );
    assert!(!primary_email.nullable);
    assert_eq!(primary_email.default, None);

    let primary_display_name = index
        .column_source("primary", "users", "display_name")
        .expect("primary users.display_name should be loaded");
    assert_eq!(
        primary_display_name.database_type.to_ascii_lowercase(),
        "text"
    );
    assert!(primary_display_name.nullable);
    assert_eq!(primary_display_name.default.as_deref(), Some("'Guest'"));

    let analytics_email = index
        .column_source("analytics", "users", "email")
        .expect("analytics users.email should be loaded independently");
    assert_eq!(
        analytics_email.database_type.to_ascii_lowercase(),
        "varchar(255)"
    );
    assert!(!analytics_email.nullable);

    let analytics_score = index
        .column_source("analytics", "users", "score")
        .expect("analytics users.score should be loaded");
    assert_eq!(
        analytics_score.database_type.to_ascii_lowercase(),
        "decimal(8,2)"
    );
    assert_eq!(analytics_score.default.as_deref(), Some("0.00"));

    let archive_archived_at = index
        .column_source("archive", "users", "archived_at")
        .expect("archive users.archived_at should be loaded");
    assert_eq!(
        archive_archived_at.database_type.to_ascii_lowercase(),
        "text"
    );
    assert_eq!(archive_archived_at.default.as_deref(), Some("null"));

    assert!(index.column_source("primary", "users", "score").is_none());
    assert!(
        index
            .column_source("analytics", "users", "display_name")
            .is_none()
    );

    assert_schema_column(
        &index,
        "pgsql_types",
        "pg_type_samples",
        "created_at",
        "TIMESTAMP WITHOUT TIME ZONE",
        false,
        Some("now()"),
    );
    assert_schema_column(
        &index,
        "pgsql_types",
        "pg_type_samples",
        "published_at",
        "TIMESTAMP WITH TIME ZONE",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "pgsql_types",
        "pg_type_samples",
        "settings",
        "JSONB",
        true,
        Some("'{}'::jsonb"),
    );
    assert_schema_column(
        &index,
        "pgsql_types",
        "pg_type_samples",
        "tags",
        "TEXT[]",
        true,
        None,
    );
    assert_schema_column(
        &index,
        "mysql_types",
        "mysql_type_samples",
        "created_at",
        "TIMESTAMP",
        true,
        Some("CURRENT_TIMESTAMP"),
    );
    assert_schema_column(
        &index,
        "mysql_types",
        "mysql_type_samples",
        "payload",
        "JSON",
        true,
        Some("null"),
    );
    assert_schema_column(
        &index,
        "sqlite_types",
        "sqlite_type_samples",
        "created_at",
        "DATETIME",
        true,
        Some("current_timestamp"),
    );

    assert_eq!(
        index
            .table("pgsql_types", "pg_type_samples")
            .unwrap()
            .columns
            .len(),
        26
    );
    assert!(
        index
            .table("mysql_types", "mysql_secondary_samples")
            .is_some()
    );
    assert!(
        index
            .table("sqlite_types", "sqlite_secondary_samples")
            .is_some()
    );
}

fn assert_schema_column(
    index: &SchemaIndex,
    connection: &str,
    table: &str,
    column: &str,
    database_type: &str,
    nullable: bool,
    default: Option<&str>,
) {
    let source = index
        .column_source(connection, table, column)
        .unwrap_or_else(|| panic!("missing {connection}.{table}.{column}"));
    assert_eq!(
        source.database_type.to_ascii_lowercase(),
        database_type.to_ascii_lowercase(),
        "{connection}.{table}.{column} database type"
    );
    assert_eq!(
        source.nullable, nullable,
        "{connection}.{table}.{column} nullability"
    );
    assert_eq!(
        source.default.as_deref().map(str::to_ascii_lowercase),
        default.map(str::to_ascii_lowercase),
        "{connection}.{table}.{column} default"
    );
}
