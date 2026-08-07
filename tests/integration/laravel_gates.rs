//! Tests for Laravel authorization gate and policy strings.
//!
//! An ability registered with `Gate::define()` in a service provider, or
//! declared as a public method of a model's policy, is surfaced wherever the
//! ability appears as a string literal: completion offers it, hover names
//! where it comes from, go-to-definition jumps to the declaration, and an
//! ability that exists nowhere — or that exists but not for the model the
//! check names — is flagged.

use crate::common::create_psr4_workspace;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

const COMPOSER_JSON: &str = r#"{
    "require": { "laravel/framework": "^11.0" },
    "autoload": { "psr-4": { "App\\": "src/" } }
}"#;

const PROVIDERS_PHP: &str = "\
<?php
return [
    App\\Providers\\AuthServiceProvider::class,
];
";

const AUTH_PROVIDER_PHP: &str = "\
<?php
namespace App\\Providers;
use App\\Models\\Video;
use App\\Policies\\LegacyVideoPolicy;
use Illuminate\\Support\\Facades\\Gate;
use Illuminate\\Support\\ServiceProvider;
class AuthServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        Gate::policy(Video::class, LegacyVideoPolicy::class);

        Gate::define('manage-billing', function (User $user, string $plan) {
            return $user->isOwner();
        });
    }
}
";

const POST_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Post extends Model {}
";

const VIDEO_PHP: &str = "\
<?php
namespace App\\Models;
use Illuminate\\Database\\Eloquent\\Model;
class Video extends Model {}
";

/// Found by the discovery convention (`App\Models\Post` →
/// `App\Policies\PostPolicy`), with no registration of its own.
const POST_POLICY_PHP: &str = "\
<?php
namespace App\\Policies;
use App\\Models\\Post;
class PostPolicy
{
    public function before($user, $ability) { return null; }
    public function viewAny($user): bool { return true; }
    public function update($user, Post $post): bool { return true; }
    protected function helper(): bool { return true; }
}
";

/// Bound to `App\Models\Video` by an explicit `Gate::policy()` call, so the
/// convention name (`App\Policies\VideoPolicy`) is never consulted.
const VIDEO_POLICY_PHP: &str = "\
<?php
namespace App\\Policies;
class LegacyVideoPolicy
{
    public function publish($user, $video): bool { return true; }
}
";

async fn open(backend: &phpantom_lsp::Backend, uri: &str, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::parse(uri).unwrap(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;
}

/// Position of the cursor immediately after the first occurrence of `needle`.
fn position_after(content: &str, needle: &str) -> Position {
    let idx = content.find(needle).expect("needle not found") + needle.len();
    let mut line = 0u32;
    let mut character = 0u32;
    for (i, ch) in content.char_indices() {
        if i == idx {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }
    Position { line, character }
}

/// Build a workspace with the provider, both models, both policies, and
/// `src/Consumer.php`.
async fn workspace(consumer: &str) -> (phpantom_lsp::Backend, tempfile::TempDir, String) {
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("src/Providers/AuthServiceProvider.php", AUTH_PROVIDER_PHP),
            ("src/Models/Post.php", POST_PHP),
            ("src/Models/Video.php", VIDEO_PHP),
            ("src/Policies/PostPolicy.php", POST_POLICY_PHP),
            ("src/Policies/LegacyVideoPolicy.php", VIDEO_POLICY_PHP),
            ("src/Consumer.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;

    let uri = Url::from_file_path(dir.path().join("src/Consumer.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;
    (backend, dir, uri)
}

async fn hover_at(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    position: Position,
) -> Option<String> {
    let hover = backend
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()?;
    match hover.contents {
        HoverContents::Markup(markup) => Some(markup.value),
        HoverContents::Scalar(MarkedString::String(s)) => Some(s),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => Some(ls.value),
        HoverContents::Array(items) => Some(
            items
                .into_iter()
                .map(|item| match item {
                    MarkedString::String(s) => s,
                    MarkedString::LanguageString(ls) => ls.value,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    }
}

async fn definition_uris(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    position: Position,
) -> Vec<String> {
    let result = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    match result {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc.uri.to_string()],
        Some(GotoDefinitionResponse::Array(locs)) => {
            locs.into_iter().map(|l| l.uri.to_string()).collect()
        }
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|l| l.target_uri.to_string())
            .collect(),
    }
}

fn ability_diagnostics(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| {
            matches!(&d.code, Some(NumberOrString::String(c)) if c == "invalid_laravel_ability")
        })
        .collect()
}

async fn completion_labels(
    backend: &phpantom_lsp::Backend,
    uri: &str,
    position: Position,
) -> Vec<String> {
    let response = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(uri).unwrap(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();
    match response {
        Some(CompletionResponse::Array(items)) => {
            items.into_iter().map(|item| item.label).collect()
        }
        Some(CompletionResponse::List(list)) => {
            list.items.into_iter().map(|item| item.label).collect()
        }
        None => Vec::new(),
    }
}

// ─── Completion ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn gate_allows_completes_defined_and_policy_abilities() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Gate;
class Consumer {
    public function go(): void {
        Gate::allows('');
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let labels = completion_labels(&backend, &uri, position_after(consumer, "allows('")).await;
    assert!(
        labels.contains(&"manage-billing".to_string()),
        "should offer the Gate::define() ability, got {labels:?}"
    );
    assert!(
        labels.contains(&"update".to_string()),
        "should offer a policy method, got {labels:?}"
    );
    assert!(
        labels.contains(&"publish".to_string()),
        "should offer the registered policy's method, got {labels:?}"
    );
    assert!(
        !labels.contains(&"before".to_string()),
        "the before() hook is not an ability, got {labels:?}"
    );
    assert!(
        !labels.contains(&"helper".to_string()),
        "a protected policy method is not an ability, got {labels:?}"
    );
}

#[tokio::test]
async fn user_can_and_this_authorize_complete_abilities() {
    let consumer = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go($user, Post $post): void {
        $user->can('', $post);
        $this->authorize('', $post);
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let can_labels = completion_labels(&backend, &uri, position_after(consumer, "can('")).await;
    assert!(
        can_labels.contains(&"update".to_string()),
        "$user->can() should complete abilities, got {can_labels:?}"
    );

    let authorize_labels =
        completion_labels(&backend, &uri, position_after(consumer, "authorize('")).await;
    assert!(
        authorize_labels.contains(&"update".to_string()),
        "$this->authorize() should complete abilities, got {authorize_labels:?}"
    );
}

#[tokio::test]
async fn can_on_an_unrelated_receiver_offers_nothing() {
    // `can()` is far too ordinary a method name to treat every call as an
    // authorization check.
    let consumer = "\
<?php
namespace App;
class Consumer {
    public function go($permissionSet): void {
        $permissionSet->can('');
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let labels = completion_labels(&backend, &uri, position_after(consumer, "can('")).await;
    assert!(
        !labels.contains(&"manage-billing".to_string()),
        "an unrelated ->can() must not complete abilities, got {labels:?}"
    );
}

// ─── Hover ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn hover_on_a_defined_ability_shows_its_registration_and_signature() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Gate;
class Consumer {
    public function go(): void {
        Gate::allows('manage-billing', 'pro');
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let hover = hover_at(&backend, &uri, position_after(consumer, "allows('mana"))
        .await
        .expect("an ability should hover");
    assert!(
        hover.contains("Gate::define()"),
        "hover should name the registration, got: {hover}"
    );
    assert!(
        hover.contains("AuthServiceProvider.php"),
        "hover should name the registering file, got: {hover}"
    );
    assert!(
        hover.contains("string $plan"),
        "hover should show the callback signature, got: {hover}"
    );
}

#[tokio::test]
async fn hover_on_a_policy_ability_lists_the_policy_methods() {
    let consumer = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go($user, Post $post): void {
        $user->can('update', $post);
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let hover = hover_at(&backend, &uri, position_after(consumer, "can('upd"))
        .await
        .expect("an ability should hover");
    assert!(
        hover.contains("App\\Policies\\PostPolicy::update"),
        "hover should name the policy method, got: {hover}"
    );
}

// ─── Go to definition ────────────────────────────────────────────────────────

#[tokio::test]
async fn a_defined_ability_resolves_to_its_registration() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Gate;
class Consumer {
    public function go(): void {
        Gate::allows('manage-billing', 'pro');
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let targets = definition_uris(&backend, &uri, position_after(consumer, "allows('mana")).await;
    assert!(
        targets
            .iter()
            .any(|t| t.ends_with("/Providers/AuthServiceProvider.php")),
        "should offer the Gate::define() site, got {targets:?}"
    );
}

#[tokio::test]
async fn a_policy_ability_resolves_to_the_policy_method() {
    let consumer = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go($user, Post $post): void {
        $user->can('update', $post);
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let targets = definition_uris(&backend, &uri, position_after(consumer, "can('upd")).await;
    assert!(
        targets
            .iter()
            .any(|t| t.ends_with("/Policies/PostPolicy.php")),
        "should offer the policy method, got {targets:?}"
    );
}

#[tokio::test]
async fn an_ability_inherited_from_a_base_policy_counts() {
    // A policy extending a shared base inherits its abilities, and the
    // inherited method's own offset points into the *base* file — so the jump
    // must land on the base policy, not at that offset in the subclass.
    let consumer = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go($user, Post $post): void {
        $user->can('restore', $post);
    }
}
";
    let base_policy = "\
<?php
namespace App\\Policies;
class BasePolicy
{
    public function restore($user, $model): bool { return true; }
}
";
    let post_policy = "\
<?php
namespace App\\Policies;
use App\\Models\\Post;
class PostPolicy extends BasePolicy
{
    public function update($user, Post $post): bool { return true; }
}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("src/Providers/AuthServiceProvider.php", AUTH_PROVIDER_PHP),
            ("src/Models/Post.php", POST_PHP),
            ("src/Models/Video.php", VIDEO_PHP),
            ("src/Policies/BasePolicy.php", base_policy),
            ("src/Policies/PostPolicy.php", post_policy),
            ("src/Policies/LegacyVideoPolicy.php", VIDEO_POLICY_PHP),
            ("src/Consumer.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("src/Consumer.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    assert!(
        ability_diagnostics(&diags).is_empty(),
        "an inherited ability must not be flagged, got {:?}",
        ability_diagnostics(&diags)
    );

    let targets = definition_uris(&backend, &uri, position_after(consumer, "can('res")).await;
    assert!(
        targets
            .iter()
            .any(|t| t.ends_with("/Policies/BasePolicy.php")),
        "should offer the declaring base policy, got {targets:?}"
    );
    assert!(
        !targets
            .iter()
            .any(|t| t.ends_with("/Policies/PostPolicy.php")),
        "must not point at an offset inside the subclass, got {targets:?}"
    );
}

// ─── Diagnostics ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn an_unknown_ability_is_flagged() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Gate;
class Consumer {
    public function go(): void {
        Gate::allows('manage-billing', 'pro');
        Gate::allows('manage-billling', 'pro');
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    let flagged = ability_diagnostics(&diags);
    assert_eq!(
        flagged.len(),
        1,
        "only the typo should be flagged, got {flagged:?}"
    );
    assert!(
        flagged[0].message.contains("manage-billling"),
        "message should name the bad ability, got {:?}",
        flagged[0].message
    );
}

#[tokio::test]
async fn an_ability_of_another_model_is_flagged_for_the_model_checked() {
    // `publish` is a real ability — but of `Video`, not `Post`.
    let consumer = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go($user, Post $post): void {
        $user->can('update', $post);
        $user->can('publish', $post);
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    let flagged = ability_diagnostics(&diags);
    assert_eq!(
        flagged.len(),
        1,
        "only the wrong-model ability should be flagged, got {flagged:?}"
    );
    assert!(
        flagged[0].message.contains("publish")
            && flagged[0].message.contains("App\\Models\\Post")
            && flagged[0].message.contains("PostPolicy"),
        "message should name the ability, the model, and its policy, got {:?}",
        flagged[0].message
    );
}

#[tokio::test]
async fn a_model_class_literal_selects_the_registered_policy() {
    // `Gate::policy()` binds `Video` to `LegacyVideoPolicy`, so `publish` is
    // valid there while `update` (a `Post` ability) is not.
    let consumer = "\
<?php
namespace App;
use App\\Models\\Video;
use Illuminate\\Support\\Facades\\Gate;
class Consumer {
    public function go(): void {
        Gate::allows('publish', Video::class);
        Gate::allows('update', Video::class);
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    let flagged = ability_diagnostics(&diags);
    assert_eq!(
        flagged.len(),
        1,
        "only the ability the video policy lacks should be flagged, got {flagged:?}"
    );
    assert!(
        flagged[0].message.contains("update") && flagged[0].message.contains("LegacyVideoPolicy"),
        "message should name the registered policy, got {:?}",
        flagged[0].message
    );
}

#[tokio::test]
async fn a_gate_definition_satisfies_a_model_bound_check() {
    // `Gate::define()` abilities are not tied to a model, so checking one
    // against a model is fine.
    let consumer = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go($user, Post $post): void {
        $user->can('manage-billing', $post);
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    assert!(
        ability_diagnostics(&diags).is_empty(),
        "a defined ability must not be flagged, got {:?}",
        ability_diagnostics(&diags)
    );
}

#[tokio::test]
async fn the_gate_define_registration_itself_is_never_flagged() {
    let provider_uri = |dir: &tempfile::TempDir| {
        Url::from_file_path(dir.path().join("src/Providers/AuthServiceProvider.php"))
            .unwrap()
            .to_string()
    };
    let consumer = "<?php\nnamespace App;\nclass Consumer {}\n";
    let (backend, dir, _uri) = workspace(consumer).await;

    let uri = provider_uri(&dir);
    open(&backend, &uri, AUTH_PROVIDER_PHP).await;
    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, AUTH_PROVIDER_PHP, &mut diags);
    assert!(
        ability_diagnostics(&diags).is_empty(),
        "the declaration site declares the ability, got {:?}",
        ability_diagnostics(&diags)
    );
}

#[tokio::test]
async fn a_use_policy_attribute_overrides_the_convention() {
    let consumer = "\
<?php
namespace App;
use App\\Models\\Post;
class Consumer {
    public function go($user, Post $post): void {
        $user->can('publish', $post);
    }
}
";
    let post_with_attribute = "\
<?php
namespace App\\Models;
use App\\Policies\\LegacyVideoPolicy;
use Illuminate\\Database\\Eloquent\\Attributes\\UsePolicy;
use Illuminate\\Database\\Eloquent\\Model;
#[UsePolicy(LegacyVideoPolicy::class)]
class Post extends Model {}
";
    let (backend, dir) = create_psr4_workspace(
        COMPOSER_JSON,
        &[
            ("bootstrap/providers.php", PROVIDERS_PHP),
            ("src/Providers/AuthServiceProvider.php", AUTH_PROVIDER_PHP),
            ("src/Models/Post.php", post_with_attribute),
            ("src/Models/Video.php", VIDEO_PHP),
            ("src/Policies/PostPolicy.php", POST_POLICY_PHP),
            ("src/Policies/LegacyVideoPolicy.php", VIDEO_POLICY_PHP),
            ("src/Consumer.php", consumer),
        ],
    );
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("src/Consumer.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    assert!(
        ability_diagnostics(&diags).is_empty(),
        "the attribute binds Post to LegacyVideoPolicy, which declares publish(), got {:?}",
        ability_diagnostics(&diags)
    );
}

#[tokio::test]
async fn the_can_middleware_parameter_is_checked() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Route;
class Consumer {
    public function go(): void {
        Route::middleware('can:update,post');
        Route::middleware('can:updatte,post');
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    let flagged = ability_diagnostics(&diags);
    assert_eq!(
        flagged.len(),
        1,
        "only the typo should be flagged, got {flagged:?}"
    );
    assert!(
        flagged[0].message.contains("updatte"),
        "message should name the bad ability, got {:?}",
        flagged[0].message
    );
    // The span covers only the ability, not the whole middleware string.
    let line = consumer
        .lines()
        .nth(flagged[0].range.start.line as usize)
        .unwrap();
    assert_eq!(
        &line[flagged[0].range.start.character as usize..flagged[0].range.end.character as usize],
        "updatte"
    );
}

#[tokio::test]
async fn a_project_with_no_abilities_is_left_alone() {
    // Without any gate registration or policy class the valid set is unknown,
    // not empty, so nothing may be flagged.
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Gate;
class Consumer {
    public function go(): void {
        Gate::allows('anything-at-all');
    }
}
";
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &[("src/Consumer.php", consumer)]);
    backend.initialized(InitializedParams {}).await;
    let uri = Url::from_file_path(dir.path().join("src/Consumer.php"))
        .unwrap()
        .to_string();
    open(&backend, &uri, consumer).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, consumer, &mut diags);
    assert!(
        ability_diagnostics(&diags).is_empty(),
        "an empty ability set means unknown, not invalid, got {:?}",
        ability_diagnostics(&diags)
    );
}

// ─── Blade ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_can_blade_directive_checks_its_ability() {
    let consumer = "<?php\nnamespace App;\nclass Consumer {}\n";
    let (backend, dir, _uri) = workspace(consumer).await;

    let template = "@can('update', $post)\n  ok\n@endcan\n@can('updatte', $post)\n  no\n@endcan\n";
    let path = dir.path().join("resources/views/post.blade.php");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, template).unwrap();
    let uri = Url::from_file_path(&path).unwrap().to_string();
    open(&backend, &uri, template).await;

    let mut diags = Vec::new();
    backend.collect_slow_diagnostics(&uri, template, &mut diags);
    let flagged = ability_diagnostics(&diags);
    assert_eq!(
        flagged.len(),
        1,
        "only the typo should be flagged, got {flagged:?}"
    );
    assert!(
        flagged[0].message.contains("updatte"),
        "message should name the bad ability, got {:?}",
        flagged[0].message
    );
    // The synthetic call the directive lowers to must not leak out as an
    // unknown function.
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("blade_can_directive")),
        "the lowered call must not surface in diagnostics, got {diags:?}"
    );
}

// ─── References ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn find_references_links_every_ability_usage() {
    let consumer = "\
<?php
namespace App;
use Illuminate\\Support\\Facades\\Gate;
class Consumer {
    public function go($user): void {
        Gate::allows('manage-billing', 'pro');
        $user->cannot('manage-billing', 'pro');
    }
}
";
    let (backend, _dir, uri) = workspace(consumer).await;

    let references = backend
        .references(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::parse(&uri).unwrap(),
                },
                position: position_after(consumer, "allows('mana"),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
        .unwrap_or_default();

    let in_consumer = references
        .iter()
        .filter(|location| location.uri.to_string() == uri)
        .count();
    assert_eq!(
        in_consumer, 2,
        "both usages should be found, got {references:?}"
    );
    assert!(
        references
            .iter()
            .any(|location| location.uri.as_str().ends_with("AuthServiceProvider.php")),
        "the Gate::define() declaration should be included, got {references:?}"
    );
}
