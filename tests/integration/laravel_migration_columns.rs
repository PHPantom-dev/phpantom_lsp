//! Model columns read statically out of `database/migrations`: which
//! Blueprint calls define a column, which only reference one (indexes,
//! foreign keys, table options), and how later migrations reshape a table.
//!
//! Cases adapted from laravel-lsp's MIT-licensed test suite.

use crate::common::{
    LARAVEL_APP_COMPOSER, complete_at_opened, create_psr4_workspace, markup_hover_at,
    open_initialized_php, position_of,
};
use tower_lsp::lsp_types::Url;

const DATABASE_CONFIG_PHP: &str = "\
<?php
return [
    'default' => 'mysql',
];
";

const USER_MODEL_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class User extends Model {}
";

const POST_MODEL_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Post extends Model {}
";

const FLAG_MODEL_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Flag extends Model {}
";

/// Wrap a `Schema::…` statement in an anonymous migration's `up()`.
fn migration(body: &str) -> String {
    format!(
        "<?php
use Illuminate\\Database\\Migrations\\Migration;
use Illuminate\\Database\\Schema\\Blueprint;
use Illuminate\\Support\\Facades\\Schema;

return new class extends Migration
{{
    public function up(): void
    {{
{body}
    }}
}};
"
    )
}

/// Lay out a Laravel app with `migrations` (file name, source), the three
/// models, and `consumer` at `app/Consumer.php`, then run the startup scan
/// and open the consumer.
async fn workspace(
    migrations: &[(&str, String)],
    consumer: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, Url) {
    let paths: Vec<String> = migrations
        .iter()
        .map(|(name, _)| format!("database/migrations/{name}"))
        .collect();
    let mut files: Vec<(&str, &str)> = vec![
        ("config/database.php", DATABASE_CONFIG_PHP),
        ("app/Models/User.php", USER_MODEL_PHP),
        ("app/Models/Post.php", POST_MODEL_PHP),
        ("app/Models/Flag.php", FLAG_MODEL_PHP),
        ("app/Consumer.php", consumer),
    ];
    for (path, (_, source)) in paths.iter().zip(migrations) {
        files.push((path.as_str(), source.as_str()));
    }
    let (backend, dir) = create_psr4_workspace(LARAVEL_APP_COMPOSER, &files);
    let uri = open_initialized_php(&backend, "app/Consumer.php").await;
    (backend, dir, uri)
}

/// The hover on the member named right after the first `access` in
/// `consumer` (e.g. `"$user->email;"`).
async fn hover_member(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    consumer: &str,
    access: &str,
) -> String {
    let arrow = access.find("->").expect("access needs ->") + 2;
    let start = position_of(consumer, access);
    markup_hover_at(backend, uri, start.line, start.character + arrow as u32).await
}

/// The names completion offers at the end of the first `line` of
/// `consumer` that ends in `->` (e.g. `"$user->\n"`).
async fn properties_after(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    consumer: &str,
    line: &str,
) -> Vec<String> {
    let start = position_of(consumer, line);
    let character = start.character + line.trim_end_matches('\n').len() as u32;
    complete_at_opened(backend, uri, start.line, character)
        .await
        .into_iter()
        .map(|item| item.filter_text.unwrap_or(item.label))
        .collect()
}

const CREATE_USERS: &str = "        Schema::create('users', function (Blueprint $table) {
            $table->id();
            $table->string('email')->unique();
            $table->string('name');
            $table->boolean('is_active')->default(true);
            $table->timestamp('email_verified_at')->nullable();
            $table->index('email');
            $table->timestamps();
        });";

const USER_CONSUMER: &str = "\
<?php
namespace App;
use App\\Models\\User;
class Consumer {
    public function go(User $user): void {
        $user->email;
        $user->email_verified_at;
        $user->is_active;
        $user->
    }
}
";

#[tokio::test]
async fn columns_of_a_create_migration_carry_their_database_types() {
    let (backend, _dir, uri) = workspace(
        &[(
            "2024_01_01_000000_create_users_table.php",
            migration(CREATE_USERS),
        )],
        USER_CONSUMER,
    )
    .await;

    let active = hover_member(&backend, &uri, USER_CONSUMER, "$user->is_active;").await;
    assert!(
        active.contains("type: `BOOLEAN`"),
        "is_active is a boolean column: {active}"
    );
    let verified = hover_member(&backend, &uri, USER_CONSUMER, "$user->email_verified_at;").await;
    assert!(
        verified.contains("type: `TIMESTAMP`") && verified.contains("nullable: `yes`"),
        "email_verified_at is a nullable timestamp: {verified}"
    );
}

/// `email` and `email_verified_at` share a prefix but are separate
/// columns, each with its own type.
#[tokio::test]
async fn a_column_sharing_a_prefix_with_another_keeps_its_own_definition() {
    let (backend, _dir, uri) = workspace(
        &[(
            "2024_01_01_000000_create_users_table.php",
            migration(CREATE_USERS),
        )],
        USER_CONSUMER,
    )
    .await;

    let email = hover_member(&backend, &uri, USER_CONSUMER, "$user->email;").await;
    assert!(
        email.contains("nullable: `no`") && !email.contains("TIMESTAMP"),
        "email is a non-null string column: {email}"
    );
}

/// `$table->index('email')` names an existing column for an index; it is
/// not a column definition and must not replace the `string('email')`
/// before it.
#[tokio::test]
async fn an_index_call_does_not_redefine_the_column_it_names() {
    let (backend, _dir, uri) = workspace(
        &[(
            "2024_01_01_000000_create_users_table.php",
            migration(CREATE_USERS),
        )],
        USER_CONSUMER,
    )
    .await;

    let email = hover_member(&backend, &uri, USER_CONSUMER, "$user->email;").await;
    assert!(
        email.contains("type: `VARCHAR`"),
        "email is still the string column after index('email'): {email}"
    );
}

const POST_CONSUMER: &str = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go(Post $post): void {
        $post->author_id;
        $post->summary;
        $post->slug;
    }
}
";

/// A foreign key constraint names its column; the column keeps the type
/// its own definition gave it.
#[tokio::test]
async fn a_foreign_key_constraint_does_not_retype_its_column() {
    let body = "        Schema::create('posts', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('author_id');
            $table->foreign('author_id')->references('id')->on('users');
        });";
    let (backend, _dir, uri) = workspace(
        &[("2024_01_01_000000_create_posts_table.php", migration(body))],
        POST_CONSUMER,
    )
    .await;

    let author = hover_member(&backend, &uri, POST_CONSUMER, "$post->author_id;").await;
    assert!(
        author.contains("type: `BIGINT`"),
        "author_id stays an unsigned big integer: {author}"
    );
}

/// Index maintenance on an existing table (`index`, `unique`,
/// `dropIndex`) adds no columns, whatever string it is handed.
#[tokio::test]
async fn index_calls_on_an_altered_table_add_no_columns() {
    let alter = "        Schema::table('users', function (Blueprint $table) {
            $table->index('legacy_code');
            $table->unique('nickname');
            $table->dropIndex('users_email_index');
        });";
    let (backend, _dir, uri) = workspace(
        &[
            (
                "2024_01_01_000000_create_users_table.php",
                migration(CREATE_USERS),
            ),
            ("2024_02_01_000000_index_users.php", migration(alter)),
        ],
        USER_CONSUMER,
    )
    .await;

    let names = properties_after(&backend, &uri, USER_CONSUMER, "$user->\n").await;
    assert!(
        names.iter().any(|n| n == "email"),
        "the real columns are still there: {names:?}"
    );
    for phantom in ["legacy_code", "nickname", "users_email_index"] {
        assert!(
            !names.iter().any(|n| n == phantom),
            "{phantom} is an index argument, not a column: {names:?}"
        );
    }
}

/// `$table->comment('…')` sets the table's comment; its text is not a
/// column.
#[tokio::test]
async fn a_table_comment_is_not_a_column() {
    let body = "        Schema::create('users', function (Blueprint $table) {
            $table->id();
            $table->string('email');
            $table->comment('Registered users');
        });";
    let (backend, _dir, uri) = workspace(
        &[("2024_01_01_000000_create_users_table.php", migration(body))],
        USER_CONSUMER,
    )
    .await;

    let names = properties_after(&backend, &uri, USER_CONSUMER, "$user->\n").await;
    assert!(
        names.iter().any(|n| n == "email"),
        "the real column is still there: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains("Registered")),
        "a table comment is not a column: {names:?}"
    );
}

/// The facade can be written fully qualified instead of imported.
#[tokio::test]
async fn a_fully_qualified_schema_facade_call_is_read() {
    let source = "<?php
return new class extends \\Illuminate\\Database\\Migrations\\Migration
{
    public function up(): void
    {
        \\Illuminate\\Support\\Facades\\Schema::create('flags', function ($table) {
            $table->boolean('enabled');
        });
    }
};
"
    .to_string();
    let consumer = "\
<?php
namespace App;
use App\\Models\\Flag;
class Consumer {
    public function go(Flag $flag): void {
        $flag->enabled;
    }
}
";
    let (backend, _dir, uri) = workspace(
        &[("2024_01_01_000000_create_flags_table.php", source)],
        consumer,
    )
    .await;

    let enabled = hover_member(&backend, &uri, consumer, "$flag->enabled;").await;
    assert!(
        enabled.contains("type: `BOOLEAN`"),
        "the column from the fully-qualified Schema call should be known: {enabled}"
    );
}

fn create_posts_then_alter() -> Vec<(&'static str, String)> {
    let create = "        Schema::create('posts', function (Blueprint $table) {
            $table->id();
            $table->unsignedBigInteger('author_id');
            $table->string('summary');
        });";
    let alter = "        Schema::table('posts', function (Blueprint $table) {
            $table->text('summary')->change();
            $table->string('slug');
        });";
    vec![
        (
            "2024_01_01_000000_create_posts_table.php",
            migration(create),
        ),
        ("2024_02_01_000000_alter_posts_table.php", migration(alter)),
    ]
}

/// `->change()` in a later migration redefines the column, so its type is
/// the one the change gave it.
#[tokio::test]
async fn a_later_change_migration_retypes_the_column() {
    let (backend, _dir, uri) = workspace(&create_posts_then_alter(), POST_CONSUMER).await;

    let summary = hover_member(&backend, &uri, POST_CONSUMER, "$post->summary;").await;
    assert!(
        summary.contains("type: `TEXT`"),
        "summary was changed to text: {summary}"
    );
}

/// `Schema::table()` adds a column to a table an earlier migration created.
#[tokio::test]
async fn a_column_added_by_a_later_schema_table_reaches_the_model() {
    let (backend, _dir, uri) = workspace(&create_posts_then_alter(), POST_CONSUMER).await;

    let slug = hover_member(&backend, &uri, POST_CONSUMER, "$post->slug;").await;
    assert!(
        slug.contains("type: `VARCHAR`"),
        "slug was added by the alter migration: {slug}"
    );
}
