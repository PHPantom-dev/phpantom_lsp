//! Integration tests for typing `validated()` from validation rules.
//!
//! The rules array names every key a validated request can carry and says
//! what each one is, so `$request->validated()` resolves to an `array{…}`
//! shape rather than plain `array`.  These tests drive it through hover and
//! completion, the two surfaces a user actually sees it on.

use crate::common::{
    FORM_REQUEST_STUB, complete_labels_at_opened, create_psr4_workspace, hover_text_at,
    open_php_at, split_cursor,
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
            "Illuminate\\Support\\Facades\\": "vendor/illuminate/Support/Facades/",
            "Illuminate\\Validation\\": "vendor/illuminate/Validation/",
            "Illuminate\\Contracts\\Validation\\": "vendor/illuminate/Contracts/Validation/"
        }
    }
}"#;

const REQUEST_PHP: &str = "\
<?php
namespace Illuminate\\Http;
use Illuminate\\Support\\ValidatedInput;
class Request {
    public function input($key = null, $default = null) { return null; }
    public function all(): array { return []; }
    public function only($keys): array { return []; }
    public function validate(array $rules): array { return []; }
    /** @return array<string, mixed> */
    public function validated($key = null, $default = null) { return []; }
    public function safe(): ValidatedInput { return new ValidatedInput(); }
}
";

const UPLOADED_FILE_PHP: &str = "\
<?php
namespace Illuminate\\Http;
class UploadedFile {
    public function store($path): string { return ''; }
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

const VALIDATOR_CONTRACT_PHP: &str = "\
<?php
namespace Illuminate\\Contracts\\Validation;
interface Validator {
    public function validated(): array;
}
";

const VALIDATOR_PHP: &str = "\
<?php
namespace Illuminate\\Validation;
use Illuminate\\Contracts\\Validation\\Validator as ValidatorContract;
class Validator implements ValidatorContract {
    public function validated(): array { return []; }
}
";

const VALIDATOR_FACADE_PHP: &str = "\
<?php
namespace Illuminate\\Support\\Facades;
use Illuminate\\Validation\\Validator;
class Validator {
    public static function make(array $data, array $rules): \\Illuminate\\Validation\\Validator {
        return new \\Illuminate\\Validation\\Validator();
    }
}
";

const STORE_POST_REQUEST_PHP: &str = "\
<?php
namespace App\\Http\\Requests;
use Illuminate\\Foundation\\Http\\FormRequest;
class StorePostRequest extends FormRequest {
    public function rules(): array {
        return [
            'title' => 'required|string|max:255',
            'views' => 'required|integer',
            'summary' => 'nullable|string',
            'draft' => 'boolean',
            'cover' => 'required|image',
            'tags' => 'required|array',
            'tags.*' => 'string',
            'author.email' => 'required|email',
        ];
    }
}
";

const ENUM_RULE_PHP: &str = "\
<?php
namespace Illuminate\\Validation\\Rules;
class Enum {
    public function __construct(string $type) {}
}
";

const RULE_FACADE_PHP: &str = "\
<?php
namespace Illuminate\\Validation;
use Illuminate\\Validation\\Rules\\Enum;
class Rule {
    public static function enum(string $type): Enum { return new Enum($type); }
}
";

const ROLE_ENUM_PHP: &str = "\
<?php
namespace App\\Enums;
enum Role: string {
    case Admin = 'admin';
    case Guest = 'guest';
}
";

const PRIORITY_ENUM_PHP: &str = "\
<?php
namespace App\\Enums;
enum Priority: int {
    case Low = 1;
    case High = 2;
}
";

const SUIT_ENUM_PHP: &str = "\
<?php
namespace App\\Enums;
enum Suit {
    case Hearts;
    case Spades;
}
";

/// A form request whose enum rules are written in its own file's terms: the
/// controller that calls it imports none of these enums.
const STORE_TICKET_REQUEST_PHP: &str = "\
<?php
namespace App\\Http\\Requests;
use App\\Enums\\Priority;
use App\\Enums\\Role;
use App\\Enums\\Suit;
use Illuminate\\Foundation\\Http\\FormRequest;
use Illuminate\\Validation\\Rule;
use Illuminate\\Validation\\Rules\\Enum;
class StoreTicketRequest extends FormRequest {
    public function rules(): array {
        return [
            'role' => ['required', new Enum(Role::class)],
            'priority' => ['required', Rule::enum(Priority::class)],
            'suit' => ['required', new Enum(Suit::class)],
        ];
    }
}
";

/// A project's own validator, to prove the receiver test is a subtype walk
/// rather than a match on the framework's two FQNs.
const APP_VALIDATOR_PHP: &str = "\
<?php
namespace App;
use Illuminate\\Validation\\Validator;
class AppValidator extends Validator {}
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

fn base_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("vendor/illuminate/Http/Request.php", REQUEST_PHP),
        ("vendor/illuminate/Http/UploadedFile.php", UPLOADED_FILE_PHP),
        (
            "vendor/illuminate/Foundation/Http/FormRequest.php",
            FORM_REQUEST_STUB,
        ),
        (
            "vendor/illuminate/Support/ValidatedInput.php",
            VALIDATED_INPUT_PHP,
        ),
        ("vendor/illuminate/Validation/Validator.php", VALIDATOR_PHP),
        (
            "vendor/illuminate/Contracts/Validation/Validator.php",
            VALIDATOR_CONTRACT_PHP,
        ),
        (
            "vendor/illuminate/Support/Facades/Validator.php",
            VALIDATOR_FACADE_PHP,
        ),
        ("vendor/illuminate/Validation/Rules/Enum.php", ENUM_RULE_PHP),
        ("vendor/illuminate/Validation/Rule.php", RULE_FACADE_PHP),
        (
            "src/Http/Requests/StorePostRequest.php",
            STORE_POST_REQUEST_PHP,
        ),
        (
            "src/Http/Requests/StoreTicketRequest.php",
            STORE_TICKET_REQUEST_PHP,
        ),
        (
            "src/Http/Requests/UpdatePostRequest.php",
            UPDATE_POST_REQUEST_PHP,
        ),
        ("src/Enums/Role.php", ROLE_ENUM_PHP),
        ("src/Enums/Priority.php", PRIORITY_ENUM_PHP),
        ("src/Enums/Suit.php", SUIT_ENUM_PHP),
        ("src/AppValidator.php", APP_VALIDATOR_PHP),
    ]
}

// ─── Harness ────────────────────────────────────────────────────────────────

/// Open `content` (cursor marked `§`) and return the hover text there.
async fn hover_text(content: &str) -> String {
    let (backend, _dir, uri, position) = open_at_cursor(content).await;

    hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default()
}

/// Open `content` (cursor marked `§`) and return the completion labels there.
async fn complete_labels(content: &str) -> Vec<String> {
    let (backend, _dir, uri, position) = open_at_cursor(content).await;
    complete_labels_at_opened(&backend, &uri, position.line, position.character).await
}

async fn open_at_cursor(
    content: &str,
) -> (phpantom_lsp::Backend, tempfile::TempDir, Url, Position) {
    let (stripped, position) = split_cursor(content);

    let mut files = base_files();
    files.push(("src/PostController.php", stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);

    let uri = open_php_at(&backend, &dir, "src/PostController.php", &stripped).await;

    (backend, dir, uri, position)
}

/// Wrap a controller body in the namespace and imports every test needs.
fn controller(body: &str) -> String {
    format!(
        "<?php
namespace App;
use App\\Enums\\Priority;
use App\\Http\\Requests\\StorePostRequest;
use App\\Http\\Requests\\StoreTicketRequest;
use App\\Http\\Requests\\UpdatePostRequest;
use Illuminate\\Http\\Request;
use Illuminate\\Support\\Facades\\Validator;
use Illuminate\\Validation\\Rules\\Enum;
class PostController {{
{body}
}}
"
    )
}

// ─── `validated()` shapes ───────────────────────────────────────────────────

#[tokio::test]
async fn form_request_validated_hovers_as_an_array_shape() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("array{"),
        "expected an array shape, got: {hover}"
    );
    assert!(
        hover.contains("title: string"),
        "expected `title: string`, got: {hover}"
    );
    assert!(
        hover.contains("views: int"),
        "expected `views: int`, got: {hover}"
    );
}

/// The keys a child request inherits through `array_merge(parent::rules(), …)`
/// belong in the shape alongside the ones it adds.
#[tokio::test]
async fn inherited_rules_are_part_of_the_shape() {
    let source = controller(
        "    public function update(UpdatePostRequest $request) {
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("slug: string"),
        "expected the child's own `slug: string`, got: {hover}"
    );
    assert!(
        hover.contains("title: string"),
        "expected the inherited `title: string`, got: {hover}"
    );
}

#[tokio::test]
async fn optionality_follows_required_and_nullable() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    // `draft` is neither required nor nullable, so it may be absent.
    assert!(
        hover.contains("draft?: bool"),
        "expected `draft?: bool`, got: {hover}"
    );
    // `summary` is nullable but not required: it may be absent, and when
    // present it may be null.
    assert!(
        hover.contains("summary?: ?string"),
        "expected `summary?: ?string`, got: {hover}"
    );
}

#[tokio::test]
async fn wildcard_rules_become_a_list() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("tags: list<string>"),
        "expected `tags: list<string>`, got: {hover}"
    );
}

#[tokio::test]
async fn shape_keys_complete_on_array_access() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $data = $request->validated();
        $data['§'];
    }",
    );
    let labels = complete_labels(&source).await;
    assert!(
        labels.contains(&"title".to_string()) && labels.contains(&"views".to_string()),
        "expected the shape's keys to complete, got: {labels:?}"
    );
}

#[tokio::test]
async fn a_file_rule_resolves_to_uploaded_file_members() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $data = $request->validated();
        $data['cover']->§
    }",
    );
    let labels = complete_labels(&source).await;
    assert!(
        labels.iter().any(|l| l.starts_with("store")),
        "expected UploadedFile members on an `image` rule, got: {labels:?}"
    );
}

// ─── Call-site rules ────────────────────────────────────────────────────────

#[tokio::test]
async fn validate_returns_the_shape_of_its_own_argument() {
    let source = controller(
        "    public function store(Request $request) {
        $data§ = $request->validate([
            'slug' => 'required|string',
            'rank' => 'required|integer',
        ]);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("slug: string") && hover.contains("rank: int"),
        "expected the argument's shape, got: {hover}"
    );
}

#[tokio::test]
async fn validator_validated_uses_the_rules_it_was_made_with() {
    let source = controller(
        "    public function store(Request $request) {
        $validator = Validator::make($request->all(), ['nickname' => 'required|string']);
        $clean§ = $validator->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("nickname: string"),
        "expected the Validator::make rules, got: {hover}"
    );
}

#[tokio::test]
async fn validated_with_a_key_returns_that_members_type() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $views§ = $request->validated('views');
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("int"),
        "expected the `views` member type, got: {hover}"
    );
}

#[tokio::test]
async fn safe_only_narrows_the_shape() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $subset§ = $request->safe()->only(['title', 'views']);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("title: string") && hover.contains("views: int"),
        "expected the narrowed shape, got: {hover}"
    );
    assert!(
        !hover.contains("draft"),
        "`only()` should drop unlisted keys, got: {hover}"
    );
}

#[tokio::test]
async fn safe_except_drops_the_listed_keys() {
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $subset§ = $request->safe()->except(['title']);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("title:"),
        "`except()` should drop the listed key, got: {hover}"
    );
    assert!(
        hover.contains("views: int"),
        "`except()` should keep the rest, got: {hover}"
    );
}

#[tokio::test]
async fn safe_narrowing_survives_being_parked_in_a_variable() {
    // The chained and two-step forms name the same request, so hover must
    // give the same shape for both.
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $safe = $request->safe();
        $subset§ = $safe->only([\'title\']);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("title: string"),
        "expected the narrowed shape through a `safe()` variable, got: {hover}"
    );
    assert!(
        !hover.contains("views"),
        "`only()` should drop unlisted keys, got: {hover}"
    );
}

#[tokio::test]
async fn a_custom_validator_subclass_is_recognised() {
    // The receiver test is a subtype walk, so a project's own validator
    // resolves its rules just like the framework's does.
    let source = controller(
        "    public function store(Request $request) {
        $validator = Validator::make($request->all(), [\'nickname\' => \'required|string\']);
        $custom = new \\App\\AppValidator();
        $clean§ = $custom->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("nickname: string"),
        "expected a validator subclass to resolve the rules, got: {hover}"
    );
}

// ─── Enum rules ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn an_enum_rule_types_its_field_as_the_enums_backing_type() {
    // The validated array holds the raw input, so a `string`-backed enum
    // validates a string and an `int`-backed one an int.  The enums are named
    // in the form request's own imports, which the controller does not share.
    let source = controller(
        "    public function store(StoreTicketRequest $request) {
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("role: string"),
        "expected `role: string` from a string-backed enum, got: {hover}"
    );
    assert!(
        hover.contains("priority: int"),
        "expected `priority: int` from an int-backed enum, got: {hover}"
    );
}

#[tokio::test]
async fn a_pure_enum_rule_claims_nothing() {
    // A non-backed enum has no raw scalar form, so the field stays `mixed`.
    let source = controller(
        "    public function store(StoreTicketRequest $request) {
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("suit: mixed"),
        "expected `suit: mixed` for a pure enum, got: {hover}"
    );
}

#[tokio::test]
async fn an_enum_rule_at_the_call_site_reads_the_calling_files_imports() {
    let source = controller(
        "    public function store(Request $request) {
        $data§ = $request->validate([
            'priority' => ['required', new Enum(Priority::class)],
        ]);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        hover.contains("priority: int"),
        "expected `priority: int` from the call site's enum rule, got: {hover}"
    );
}

// ─── Falling back ───────────────────────────────────────────────────────────

#[tokio::test]
async fn a_request_with_no_visible_rules_stays_plain_array() {
    let source = controller(
        "    public function store(Request $request) {
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "no rules are visible, so no shape may be claimed: {hover}"
    );
}

#[tokio::test]
async fn an_unreadable_nearer_validate_call_does_not_reuse_an_earlier_shape() {
    // The rules in force are the second call's, and they cannot be read.
    // Describing `$data` with the first call's keys would be a shape that
    // claims to be complete while naming the wrong request's fields.
    let source = controller(
        "    public function store(Request $request) {
        $request->validate(['slug' => 'required|string']);
        $request->validate([$extra => 'required|string']);
        $data§ = $request->validated();
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "the rules in force are unreadable, so no shape may be claimed: {hover}"
    );
}

#[tokio::test]
async fn only_on_an_unrelated_receiver_keeps_its_own_return_type() {
    // `only()` and `except()` are ordinary `Collection` methods.  Narrowing
    // applies to the `ValidatedInput` that `safe()` returns and nothing else,
    // even in a file where a `safe()` call is in scope.
    let source = controller(
        "    public function store(StorePostRequest $request) {
        $safe = $request->safe();
        $bag = collect(['title' => 1]);
        $subset§ = $bag->only(['title']);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "a Collection receiver must not pick up the request's shape: {hover}"
    );
}

#[tokio::test]
async fn a_computed_rule_key_falls_back_to_plain_array() {
    let source = controller(
        "    public function store(Request $request) {
        $data§ = $request->validate([
            'slug' => 'required|string',
            $extra => 'required',
        ]);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "an incomplete key set must not become a shape: {hover}"
    );
}

#[tokio::test]
async fn validated_with_a_computed_key_is_not_the_whole_shape() {
    // `validated($key)` returns one field's value whatever the key is, so an
    // unreadable key must leave the declared type alone.  Typing it as the
    // whole array reports every use of the value as an array mismatch.
    let source = controller(
        "    public function store(StorePostRequest $request, string $key) {
        $value§ = $request->validated($key);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "a computed key selects one field, not the shape: {hover}"
    );
}

#[tokio::test]
async fn only_with_a_computed_key_list_keeps_the_declared_array() {
    // The keys narrowed to are unknown, so any shape would omit real keys and
    // report reading them as an unknown-key error.
    let source = controller(
        "    public function store(StorePostRequest $request, array $keys) {
        $subset§ = $request->safe()->only($keys);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "an unreadable key list must not narrow the shape: {hover}"
    );
}

#[tokio::test]
async fn only_with_a_partly_computed_key_list_keeps_the_declared_array() {
    let source = controller(
        "    public function store(StorePostRequest $request, string $extra) {
        $subset§ = $request->safe()->only(['title', $extra]);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "dropping the unreadable key would narrow too far: {hover}"
    );
}

#[tokio::test]
async fn except_with_a_computed_key_list_keeps_the_declared_array() {
    // Keeping every key would be safe for diagnostics but still wrong: the
    // shape would claim fields the call removed.
    let source = controller(
        "    public function store(StorePostRequest $request, array $keys) {
        $subset§ = $request->safe()->except($keys);
    }",
    );
    let hover = hover_text(&source).await;
    assert!(
        !hover.contains("array{"),
        "an unreadable key list must not produce a shape: {hover}"
    );
}
