//! Integration tests for the request input accessors' per-argument types.
//!
//! `header()`, `query()`, `cookie()`, `input()` and `file()` each declare
//! one union spanning every way of calling them, so the arguments at the
//! call site are what say which of those ways this is: a key or no key, a
//! default or no default.

use crate::common::{FORM_REQUEST_STUB, create_psr4_workspace, hover_text_at, split_cursor};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Shared stubs ───────────────────────────────────────────────────────────

const COMPOSER_JSON: &str = r#"{
    "autoload": {
        "psr-4": {
            "App\\": "src/",
            "App\\Http\\Requests\\": "src/Http/Requests/",
            "Illuminate\\Http\\": "vendor/illuminate/Http/",
            "Illuminate\\Foundation\\Http\\": "vendor/illuminate/Foundation/Http/"
        }
    }
}"#;

/// The framework's own signatures, unions and all.
const REQUEST_PHP: &str = "\
<?php
namespace Illuminate\\Http;
class Request {
    /**
     * @param  string|null  $key
     * @param  string|array|null  $default
     * @return string|array|null
     */
    public function header($key = null, $default = null) { return null; }
    /**
     * @param  string|null  $key
     * @param  string|array|null  $default
     * @return string|array|null
     */
    public function query($key = null, $default = null) { return null; }
    /**
     * @param  string|null  $key
     * @param  string|array|null  $default
     * @return string|array|null
     */
    public function cookie($key = null, $default = null) { return null; }
    /**
     * @param  string|null  $key
     * @param  mixed  $default
     * @return mixed
     */
    public function input($key = null, $default = null) { return null; }
    /**
     * @param  string|null  $key
     * @param  mixed  $default
     * @return \\Illuminate\\Http\\UploadedFile|\\Illuminate\\Http\\UploadedFile[]|array|null
     */
    public function file($key = null, $default = null) { return null; }
    public function rules(): array { return []; }
}
";

const UPLOADED_FILE_PHP: &str = "\
<?php
namespace Illuminate\\Http;
class UploadedFile {
    public function store($path): string { return ''; }
    public function getClientOriginalName(): string { return ''; }
}
";

/// One field validated as a single upload and one as a list of them, which
/// is the only thing that tells `file('…')`'s two shapes apart.
const STORE_POST_REQUEST_PHP: &str = "\
<?php
namespace App\\Http\\Requests;
use Illuminate\\Foundation\\Http\\FormRequest;
class StorePostRequest extends FormRequest {
    public function rules(): array {
        return [
            'title' => 'required|string',
            'cover' => 'required|image',
            'photos' => 'required|array',
            'photos.*' => 'required|file',
        ];
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
            "src/Http/Requests/StorePostRequest.php",
            STORE_POST_REQUEST_PHP,
        ),
    ]
}

// ─── Harness ────────────────────────────────────────────────────────────────

/// Open `content` (cursor marked `§`) and return the hover text there.
async fn hover_text(content: &str) -> String {
    let (stripped, position) = split_cursor(content);

    let mut files = base_files();
    files.push(("src/PostController.php", stripped.as_str()));
    let (backend, dir) = create_psr4_workspace(COMPOSER_JSON, &files);

    let uri = Url::from_file_path(dir.path().join("src/PostController.php")).unwrap();
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: stripped.clone(),
            },
        })
        .await;

    hover_text_at(&backend, &uri, position.line, position.character)
        .await
        .unwrap_or_default()
}

/// Wrap a controller body in the namespace and imports every test needs.
fn controller(body: &str) -> String {
    format!(
        "<?php
namespace App;

use App\\Http\\Requests\\StorePostRequest;
use Illuminate\\Http\\Request;
use Illuminate\\Http\\UploadedFile;

class PostController
{{
{body}
}}
"
    )
}

// ─── header() ───────────────────────────────────────────────────────────────

/// A header is a string, and the default is what a missing one produces, so
/// a string default leaves no other outcome.
#[tokio::test]
async fn a_header_with_a_string_default_is_a_string() {
    let hover = hover_text(&controller(
        "    public function show(Request $request): void\n\
         \x20   {\n\
         \x20       $agent = $request->header('User-Agent', '');\n\
         \x20       $ag§ent;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("string") && !hover.contains("array") && !hover.contains("null"),
        "a defaulted header is a plain string, got: {hover}"
    );
}

/// Without a default the header may be missing, which is the null.
#[tokio::test]
async fn a_header_without_a_default_is_a_nullable_string() {
    let hover = hover_text(&controller(
        "    public function show(Request $request): void\n\
         \x20   {\n\
         \x20       $agent = $request->header('User-Agent');\n\
         \x20       $ag§ent;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("string") && hover.contains("null") && !hover.contains("array"),
        "an undefaulted header is `string|null`, got: {hover}"
    );
}

/// A header never holds an array; the array in the declared union is the
/// keyless form, which returns the whole bag.
#[tokio::test]
async fn the_header_bag_is_an_array_of_header_values() {
    let hover = hover_text(&controller(
        "    public function show(Request $request): void\n\
         \x20   {\n\
         \x20       $headers = $request->header();\n\
         \x20       $head§ers;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("array<string,"),
        "the keyless form is the whole bag, got: {hover}"
    );
}

// ─── query() / cookie() ─────────────────────────────────────────────────────

/// `query()` with no key is the query array, which is what a caller
/// forwarding the whole query string needs.
#[tokio::test]
async fn the_query_bag_is_an_array() {
    let hover = hover_text(&controller(
        "    public function show(Request $request): void\n\
         \x20   {\n\
         \x20       $query = $request->query();\n\
         \x20       $qu§ery;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("array<string,") && !hover.contains("null"),
        "the keyless query form is the whole array, got: {hover}"
    );
}

/// A query parameter really can be an array (`?tags[]=a&tags[]=b`), so the
/// keyed form keeps what Laravel declares.
#[tokio::test]
async fn a_query_parameter_keeps_its_declared_union() {
    let hover = hover_text(&controller(
        "    public function show(Request $request): void\n\
         \x20   {\n\
         \x20       $tags = $request->query('tags');\n\
         \x20       $ta§gs;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("array"),
        "a query parameter may still be an array, got: {hover}"
    );
}

// ─── input() ────────────────────────────────────────────────────────────────

/// `input()` with no key is the whole input array; a JSON body makes the
/// keyed form anything at all, which is what `mixed` already says.
#[tokio::test]
async fn the_input_bag_is_an_array() {
    let hover = hover_text(&controller(
        "    public function show(Request $request): void\n\
         \x20   {\n\
         \x20       $all = $request->input();\n\
         \x20       $a§ll;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("array<string,"),
        "the keyless input form is the whole array, got: {hover}"
    );
}

// ─── file() ─────────────────────────────────────────────────────────────────

/// The rules say `cover` is one image, so the list half of the declared
/// union cannot happen.
#[tokio::test]
async fn a_file_validated_as_one_upload_is_one_upload() {
    let hover = hover_text(&controller(
        "    public function store(StorePostRequest $request): void\n\
         \x20   {\n\
         \x20       $cover = $request->file('cover');\n\
         \x20       $co§ver;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("UploadedFile") && hover.contains("null") && !hover.contains("array"),
        "a single-upload field is `UploadedFile|null`, got: {hover}"
    );
}

/// And the field validated as an array of files is the list half.
#[tokio::test]
async fn a_file_validated_as_a_list_is_a_list_of_uploads() {
    let hover = hover_text(&controller(
        "    public function store(StorePostRequest $request): void\n\
         \x20   {\n\
         \x20       $photos = $request->file('photos');\n\
         \x20       $phot§os;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("list<UploadedFile>") && hover.contains("null"),
        "a multi-upload field is a list of uploads, got: {hover}"
    );
}

/// A key nothing describes is the single upload a named field holds.
#[tokio::test]
async fn an_undescribed_file_key_is_one_upload() {
    let hover = hover_text(&controller(
        "    public function store(Request $request): void\n\
         \x20   {\n\
         \x20       $upload = $request->file('cover');\n\
         \x20       $uplo§ad;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("UploadedFile") && hover.contains("null") && !hover.contains("list<"),
        "an unvalidated key names one upload, got: {hover}"
    );
}

/// The keyless form is the whole file bag, keyed by field name.
#[tokio::test]
async fn the_file_bag_is_an_array_of_uploads() {
    let hover = hover_text(&controller(
        "    public function store(Request $request): void\n\
         \x20   {\n\
         \x20       $files = $request->file();\n\
         \x20       $fil§es;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("array<string,") && hover.contains("UploadedFile"),
        "the keyless file form is the whole bag, got: {hover}"
    );
}

/// The way a controller tells the two shapes apart itself. The guard has to
/// be able to rule the list half out, which it can only do if the class is
/// named the way the narrowing compares class names.
#[tokio::test]
async fn an_instanceof_guard_rules_out_the_list_half() {
    let hover = hover_text(&controller(
        "    public function store(Request $request): void\n\
         \x20   {\n\
         \x20       $cover = $request->file('cover');\n\
         \x20       if ($cover instanceof UploadedFile) {\n\
         \x20           $co§ver;\n\
         \x20       }\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("UploadedFile") && !hover.contains("array") && !hover.contains("null"),
        "the guard proves a single upload, got: {hover}"
    );
}

/// A member call on the narrowed upload still resolves, so the type carries
/// its class rather than travelling as a bare hint.
#[tokio::test]
async fn a_narrowed_upload_keeps_its_class() {
    let hover = hover_text(&controller(
        "    public function store(StorePostRequest $request): void\n\
         \x20   {\n\
         \x20       $name = $request->file('cover')?->getClientOriginalName();\n\
         \x20       $na§me;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("string"),
        "the upload's own members resolve, got: {hover}"
    );
}

// ─── named arguments ────────────────────────────────────────────────────────

/// `file(key: 'photos')` names its key argument, so the key still has to be
/// read as `'photos'` rather than as whichever positional slot it lands in.
#[tokio::test]
async fn a_named_key_argument_still_supplies_the_key() {
    let hover = hover_text(&controller(
        "    public function store(StorePostRequest $request): void\n\
         \x20   {\n\
         \x20       $photos = $request->file(key: 'photos');\n\
         \x20       $phot§os;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("list<UploadedFile>") && hover.contains("null"),
        "a named key argument names the same field as a positional one, got: {hover}"
    );
}

/// `header(default: 'x')` supplies only the default, so the call is still the
/// keyless form and returns the whole header bag rather than a plain string.
#[tokio::test]
async fn a_named_default_argument_alone_leaves_the_call_keyless() {
    let hover = hover_text(&controller(
        "    public function show(Request $request): void\n\
         \x20   {\n\
         \x20       $all = $request->header(default: 'x');\n\
         \x20       $a§ll;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("array<string,"),
        "a default-only call has no key, so it stays the whole bag, got: {hover}"
    );
}

/// A `FormRequest` subclass never redeclares `file()`, so its parameters have
/// to be found by walking up to `Illuminate\Http\Request` rather than reading
/// only the receiver's own members.
#[tokio::test]
async fn a_named_key_argument_resolves_through_a_form_request_subclass() {
    let hover = hover_text(&controller(
        "    public function store(StorePostRequest $request): void\n\
         \x20   {\n\
         \x20       $cover = $request->file(key: 'cover');\n\
         \x20       $co§ver;\n\
         \x20   }",
    ))
    .await;
    assert!(
        hover.contains("UploadedFile") && hover.contains("null") && !hover.contains("list<"),
        "a named key still resolves through the FormRequest subclass, got: {hover}"
    );
}
