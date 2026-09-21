//! Integration tests for request input field names recovered from
//! validation rules — completion inside `$request->input('…')` and friends,
//! plus go-to-definition from a key to the rule that declares it.

use crate::common::{
    FORM_REQUEST_STUB, complete_at_opened, create_psr4_workspace, definition_locations,
    goto_definition_at, item_labels, open_php_at, split_cursor,
};
use tower_lsp::lsp_types::*;

// ─── Shared stubs ───────────────────────────────────────────────────────────

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "App\\Http\\Requests\\": "src/Http/Requests/",
            "Illuminate\\Http\\": "vendor/illuminate/Http/",
            "Illuminate\\Foundation\\Http\\": "vendor/illuminate/Foundation/Http/",
            "Illuminate\\Support\\": "vendor/illuminate/Support/",
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/"
        }
    }
}"#;

const REQUEST_PHP: &str = "\
<?php
namespace Illuminate\\Http;
use Illuminate\\Support\\ValidatedInput;
class Request {
    public function input($key = null, $default = null) { return null; }
    public function query($key = null, $default = null) { return null; }
    public function string($key, $default = null) { return ''; }
    public function integer($key, $default = 0): int { return 0; }
    public function boolean($key = null, $default = false): bool { return false; }
    public function has($key): bool { return false; }
    public function hasAny(...$keys): bool { return false; }
    public function filled($key): bool { return false; }
    public function missing($key): bool { return false; }
    public function only($keys): array { return []; }
    public function except($keys): array { return []; }
    public function merge(array $input) { return $this; }
    public function all(): array { return []; }
    public function validate(array $rules): array { return []; }
    public function validated($key = null, $default = null) { return []; }
    public function safe(): ValidatedInput { return new ValidatedInput(); }
}
";

const VALIDATED_INPUT_PHP: &str = "\
<?php
namespace Illuminate\\Support;
class ValidatedInput {
    public function only($keys): array { return []; }
    public function except($keys): array { return []; }
}
";

const VALIDATOR_FACADE_PHP: &str = "\
<?php
namespace Illuminate\\Support\\Facades;
class Validator {
    public static function make(array $data, array $rules) { return null; }
}
";

/// A `FormRequest` whose `rules()` covers plain, dotted and wildcard keys.
const STORE_POST_REQUEST_PHP: &str = "\
<?php
namespace App\\Http\\Requests;
use Illuminate\\Foundation\\Http\\FormRequest;
class StorePostRequest extends FormRequest {
    public function rules(): array {
        return [
            'title' => 'required|string|max:255',
            'published' => 'boolean',
            'tags' => 'array',
            'tags.*.name' => 'required|string',
            'author.email' => 'required|email',
        ];
    }
}
";

/// A child request that composes its parent's rules with `array_merge`.
const UPDATE_POST_REQUEST_PHP: &str = "\
<?php
namespace App\\Http\\Requests;
class UpdatePostRequest extends StorePostRequest {
    public function rules(): array {
        return array_merge(parent::rules(), [
            'slug' => 'required|string',
        ]);
    }
}
";

/// A trait that supplies a `rules()` method to whichever request uses it.
const HAS_POST_RULES_TRAIT_PHP: &str = "\
<?php
namespace App\\Http\\Requests\\Concerns;
trait HasPostRules {
    public function rules(): array {
        return [
            'headline' => 'required|string|max:255',
            'summary' => 'nullable|string',
        ];
    }
}
";

/// A request that declares no `rules()` of its own and gets one from a trait.
const TRAIT_POST_REQUEST_PHP: &str = "\
<?php
namespace App\\Http\\Requests;
use App\\Http\\Requests\\Concerns\\HasPostRules;
use Illuminate\\Foundation\\Http\\FormRequest;
class TraitPostRequest extends FormRequest {
    use HasPostRules;
}
";

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vendor/illuminate/Http/Request.php", REQUEST_PHP),
        (
            "vendor/illuminate/Foundation/Http/FormRequest.php",
            FORM_REQUEST_STUB,
        ),
        (
            "vendor/illuminate/Support/ValidatedInput.php",
            VALIDATED_INPUT_PHP,
        ),
        (
            "vendor/illuminate/Support/Facades/Validator.php",
            VALIDATOR_FACADE_PHP,
        ),
        (
            "src/Http/Requests/StorePostRequest.php",
            STORE_POST_REQUEST_PHP,
        ),
        (
            "src/Http/Requests/UpdatePostRequest.php",
            UPDATE_POST_REQUEST_PHP,
        ),
        (
            "src/Http/Requests/Concerns/HasPostRules.php",
            HAS_POST_RULES_TRAIT_PHP,
        ),
        (
            "src/Http/Requests/TraitPostRequest.php",
            TRAIT_POST_REQUEST_PHP,
        ),
    ]
}

// ─── Harness ────────────────────────────────────────────────────────────────

/// Open `content` at `open_path` and return the completion labels at the
/// cursor, which is marked in the source by `§`.
async fn complete_labels(open_path: &str, content: &str) -> Vec<String> {
    item_labels(complete_items(open_path, content).await)
}

async fn complete_items(open_path: &str, content: &str) -> Vec<CompletionItem> {
    let (backend, _dir, uri, position) = open_at_cursor(open_path, content).await;
    complete_at_opened(&backend, &uri, position.line, position.character).await
}

/// Resolve go-to-definition at the `§` cursor and return the target URI and
/// the source line it points at.
async fn definition_line(open_path: &str, content: &str) -> Option<(String, String)> {
    let (backend, _dir, uri, position) = open_at_cursor(open_path, content).await;

    let response = goto_definition_at(&backend, &uri, position.line, position.character).await;
    let location = definition_locations(response).into_iter().next()?;

    let path = location.uri.to_file_path().ok()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let line = text.lines().nth(location.range.start.line as usize)?.trim();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some((name, line.to_string()))
}

/// Write the workspace, strip the `§` cursor marker from `content`, open the
/// file, and return everything a request needs.
async fn open_at_cursor(
    open_path: &str,
    content: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, Url, Position) {
    let (stripped, position) = split_cursor(content);

    // The opened file is also written to disk so that go-to-definition can
    // read back the line it resolves to.
    let mut files = base_files();
    files.push((open_path, stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);

    let uri = open_php_at(&backend, &dir, open_path, &stripped).await;

    (backend, dir, uri, position)
}

// ─── FormRequest-driven completion ──────────────────────────────────────────

#[tokio::test]
async fn form_request_rules_drive_input_completion() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->input('§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert!(
        labels.contains(&"title".to_string()),
        "expected 'title' from StorePostRequest::rules(), got: {labels:?}"
    );
    assert!(
        labels.contains(&"published".to_string()),
        "expected 'published', got: {labels:?}"
    );
}

/// `array_merge(parent::rules(), […])` must offer the ancestor's keys too, not
/// only the ones the child adds.
#[tokio::test]
async fn parent_rules_are_merged_into_input_completion() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\UpdatePostRequest;
class PostController {
    public function update(UpdatePostRequest $request) {
        $request->input('§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    for expected in ["slug", "title", "published", "tags"] {
        assert!(
            labels.contains(&expected.to_string()),
            "expected '{expected}' from the merged parent rules, got: {labels:?}"
        );
    }
}

/// A `rules()` method a trait supplies is the request's own method as far as
/// PHP is concerned, so its keys must reach completion.
#[tokio::test]
async fn trait_supplied_rules_drive_input_completion() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\TraitPostRequest;
class PostController {
    public function store(TraitPostRequest $request) {
        $request->input('§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    for expected in ["headline", "summary"] {
        assert!(
            labels.contains(&expected.to_string()),
            "expected '{expected}' from the trait's rules(), got: {labels:?}"
        );
    }
}

/// Go-to-definition on a trait-supplied key lands on the trait, not on the
/// class that uses it.
#[tokio::test]
async fn definition_of_a_trait_supplied_key_jumps_to_the_trait() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\TraitPostRequest;
class PostController {
    public function store(TraitPostRequest $request) {
        $request->input('head§line');
    }
}
";
    let (file, line) = definition_line("src/PostController.php", controller)
        .await
        .expect("expected a definition for the trait-supplied 'headline'");
    assert_eq!(file, "HasPostRules.php");
    assert_eq!(line, "'headline' => 'required|string|max:255',");
}

#[tokio::test]
async fn wildcard_rule_keys_complete_their_root_segment() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->input('tag§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert_eq!(
        labels,
        vec!["tags".to_string()],
        "`tags.*.name` should only be reachable through its root segment"
    );
}

#[tokio::test]
async fn dotted_rule_keys_complete_whole_and_by_root() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->input('author§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert_eq!(
        labels,
        vec!["author.email".to_string(), "author".to_string()]
    );
}

#[tokio::test]
async fn completion_detail_shows_the_rule() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->string('titl§');
    }
}
";
    let items = complete_items("src/PostController.php", controller).await;
    let title = items
        .iter()
        .find(|item| item.label == "title")
        .expect("expected a 'title' item");
    assert_eq!(title.detail.as_deref(), Some("required|string|max:255"));
}

#[tokio::test]
async fn form_request_completes_its_own_rules_through_this() {
    let request = "\
<?php
namespace App\\Http\\Requests;
use Illuminate\\Foundation\\Http\\FormRequest;
class UpdatePostRequest extends FormRequest {
    public function rules(): array {
        return ['headline' => 'required|string'];
    }
    public function withValidator(): void {
        $this->input('§');
    }
}
";
    let labels = complete_labels("src/Http/Requests/UpdatePostRequest.php", request).await;
    assert!(
        labels.contains(&"headline".to_string()),
        "expected the class's own rules through $this, got: {labels:?}"
    );
}

// ─── Inline `validate()` / `Validator::make()` ──────────────────────────────

#[tokio::test]
async fn inline_validate_call_drives_completion() {
    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class PostController {
    public function store(Request $request) {
        $request->validate([
            'slug' => 'required|string',
            'excerpt' => 'nullable|string',
        ]);
        $request->input('§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert!(
        labels.contains(&"slug".to_string()) && labels.contains(&"excerpt".to_string()),
        "expected inline validate() keys, got: {labels:?}"
    );
}

/// Which arm of an `if`/`else` ran is not knowable, so the keys of a
/// `validate()` call in either arm describe the request after it.
#[tokio::test]
async fn validate_calls_in_both_branches_both_reach_the_cursor() {
    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class PostController {
    public function store(Request $request) {
        if ($request->has('draft')) {
            $request->validate(['draft_note' => 'required|string']);
        } else {
            $request->validate(['published_at' => 'required|date']);
        }
        $request->input('§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    for expected in ["draft_note", "published_at"] {
        assert!(
            labels.contains(&expected.to_string()),
            "expected '{expected}' from the branch's validate() call, got: {labels:?}"
        );
    }
}

#[tokio::test]
async fn validator_make_drives_completion() {
    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
use Illuminate\\Support\\Facades\\Validator;
class PostController {
    public function store(Request $request) {
        Validator::make($request->all(), ['nickname' => 'required']);
        $request->input('§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert!(
        labels.contains(&"nickname".to_string()),
        "expected Validator::make() keys, got: {labels:?}"
    );
}

#[tokio::test]
async fn no_rules_in_scope_yields_no_field_completion() {
    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class PostController {
    public function store(Request $request) {
        $request->input('§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert!(
        !labels.contains(&"title".to_string()),
        "an unvalidated Request has no known keys, got: {labels:?}"
    );
}

// ─── Accessor surface ───────────────────────────────────────────────────────

#[tokio::test]
async fn array_access_completes_field_names() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $title = $request['titl§'];
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert_eq!(labels, vec!["title".to_string()]);
}

#[tokio::test]
async fn safe_only_completes_field_names() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $data = $request->safe()->only(['titl§']);
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert_eq!(labels, vec!["title".to_string()]);
}

#[tokio::test]
async fn safe_held_in_a_variable_completes_field_names() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $safe = $request->safe();
        $data = $safe->only(['titl§']);
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert_eq!(
        labels,
        vec!["title".to_string()],
        "a ValidatedInput carries the rules of the request it came from"
    );
}

#[tokio::test]
async fn safe_variable_reassignment_follows_the_nearest_request() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
use Illuminate\\Http\\Request;
class PostController {
    public function store(StorePostRequest $request, Request $other) {
        $safe = $other->safe();
        $safe = $request->safe();
        $data = $safe->only(['titl§']);
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert_eq!(
        labels,
        vec!["title".to_string()],
        "the last assignment before the cursor decides which request applies"
    );
}

#[tokio::test]
async fn safe_variable_without_a_traceable_request_completes_nothing() {
    let controller = "\
<?php
namespace App;
use Illuminate\\Support\\ValidatedInput;
class PostController {
    public function store(ValidatedInput $safe) {
        $data = $safe->only(['titl§']);
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert!(
        labels.is_empty(),
        "a ValidatedInput with no request in sight has no known keys: {labels:?}"
    );
}

#[tokio::test]
async fn has_completes_field_names() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        if ($request->has('publishe§')) {}
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert_eq!(labels, vec!["published".to_string()]);
}

#[tokio::test]
async fn unrelated_method_does_not_complete_field_names() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->merge(['titl§' => 'x']);
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert!(
        !labels.contains(&"title".to_string()),
        "merge() writes input, it does not read a validated key: {labels:?}"
    );
}

#[tokio::test]
async fn non_request_receiver_does_not_complete_field_names() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class Bag {
    public function input($key) { return null; }
}
class PostController {
    public function store(StorePostRequest $request) {
        $bag = new Bag();
        $bag->input('titl§');
    }
}
";
    let labels = complete_labels("src/PostController.php", controller).await;
    assert!(
        !labels.contains(&"title".to_string()),
        "only request objects carry validated input: {labels:?}"
    );
}

// ─── Go to definition ───────────────────────────────────────────────────────

#[tokio::test]
async fn definition_jumps_to_the_form_request_rule() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->input('tit§le');
    }
}
";
    let (file, line) = definition_line("src/PostController.php", controller)
        .await
        .expect("expected a definition for 'title'");
    assert_eq!(file, "StorePostRequest.php");
    assert_eq!(line, "'title' => 'required|string|max:255',");
}

#[tokio::test]
async fn definition_of_an_inherited_key_jumps_to_the_parent_request() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\UpdatePostRequest;
class PostController {
    public function update(UpdatePostRequest $request) {
        $request->input('tit§le');
    }
}
";
    let (file, line) = definition_line("src/PostController.php", controller)
        .await
        .expect("expected a definition for the inherited 'title'");
    assert_eq!(file, "StorePostRequest.php");
    assert_eq!(line, "'title' => 'required|string|max:255',");
}

#[tokio::test]
async fn definition_jumps_to_the_inline_rule() {
    let controller = "\
<?php
namespace App;
use Illuminate\\Http\\Request;
class PostController {
    public function store(Request $request) {
        $request->validate(['slug' => 'required|string']);
        $request->input('sl§ug');
    }
}
";
    let (file, line) = definition_line("src/PostController.php", controller)
        .await
        .expect("expected a definition for 'slug'");
    assert_eq!(file, "PostController.php");
    assert_eq!(line, "$request->validate(['slug' => 'required|string']);");
}

#[tokio::test]
async fn definition_of_a_wildcard_root_lands_on_the_wildcard_rule() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->input('ta§gs');
    }
}
";
    let (file, line) = definition_line("src/PostController.php", controller)
        .await
        .expect("expected a definition for 'tags'");
    assert_eq!(file, "StorePostRequest.php");
    assert_eq!(line, "'tags' => 'array',");
}

#[tokio::test]
async fn definition_of_an_unknown_key_resolves_nothing() {
    let controller = "\
<?php
namespace App;
use App\\Http\\Requests\\StorePostRequest;
class PostController {
    public function store(StorePostRequest $request) {
        $request->input('nope§');
    }
}
";
    assert!(
        definition_line("src/PostController.php", controller)
            .await
            .is_none(),
        "a key that no rule declares has no definition"
    );
}
