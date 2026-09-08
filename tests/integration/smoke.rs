//! Smoke tests for PHPantom's LSP features.
//!
//! These tests exercise the full LSP request lifecycle through the
//! `LanguageServer` trait, verifying that completion, hover,
//! go-to-definition, and signature help return meaningful results
//! for realistic PHP code.
//!
//! Unlike the unit/integration tests that focus on specific edge cases,
//! these tests verify that the major features work end-to-end with
//! non-trivial PHP files containing classes, inheritance, generics,
//! docblocks, and cross-method calls.

use crate::common::{create_psr4_workspace, create_test_backend, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Marker that indicates the cursor position in test PHP source.
const CURSOR: &str = "<>";

/// Strip the `<>` cursor marker from source and return the cleaned source,
/// the 0-based line number, and the 0-based character offset.
fn strip_cursor(src: &str) -> (String, u32, u32) {
    let idx = src
        .find(CURSOR)
        .unwrap_or_else(|| panic!("Test source must contain a `{CURSOR}` cursor marker"));
    let before = &src[..idx];
    let line = before.chars().filter(|&c| c == '\n').count() as u32;
    let last_nl = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = before[last_nl..].len() as u32;
    let cleaned = format!("{}{}", &src[..idx], &src[idx + CURSOR.len()..]);
    (cleaned, line, character)
}

/// Open a PHP file on the backend and return its URI plus the cursor position.
async fn open_with_cursor(
    backend: &phpantom_lsp::Backend,
    uri_str: &str,
    src_with_cursor: &str,
) -> (Url, u32, u32) {
    let (content, line, character) = strip_cursor(src_with_cursor);
    let uri = Url::parse(uri_str).unwrap();
    open_php(backend, &uri, &content).await;
    (uri, line, character)
}

/// Open a PHP file on the backend (no cursor) and return its URI.
async fn open_file(backend: &phpantom_lsp::Backend, uri_str: &str, content: &str) -> Url {
    let uri = Url::parse(uri_str).unwrap();
    open_php(backend, &uri, content).await;
    uri
}

/// Fire a completion request and return the item labels.
async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<String> {
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };
    let response = backend.completion(params).await.unwrap();
    match response {
        Some(CompletionResponse::Array(items)) => items.iter().map(|i| i.label.clone()).collect(),
        Some(CompletionResponse::List(list)) => {
            list.items.iter().map(|i| i.label.clone()).collect()
        }
        None => Vec::new(),
    }
}

/// Fire a hover request and return the hover text (if any).
async fn hover_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<String> {
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let result = backend.hover(params).await.unwrap()?;
    Some(match result.contents {
        HoverContents::Markup(mc) => mc.value,
        HoverContents::Scalar(MarkedString::String(s)) => s,
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value,
        HoverContents::Array(items) => items
            .into_iter()
            .map(|ms| match ms {
                MarkedString::String(s) => s,
                MarkedString::LanguageString(ls) => ls.value,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

/// Fire a go-to-definition request and return the locations.
async fn definition_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Vec<Location> {
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let result = backend.goto_definition(params).await.unwrap();
    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => vec![loc],
        Some(GotoDefinitionResponse::Array(locs)) => locs,
        Some(GotoDefinitionResponse::Link(links)) => links
            .into_iter()
            .map(|link| Location {
                uri: link.target_uri,
                range: link.target_selection_range,
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Fire a signature help request and return the result.
async fn signature_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    line: u32,
    character: u32,
) -> Option<SignatureHelp> {
    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        context: None,
    };
    backend.signature_help(params).await.unwrap()
}

// ─── Full lifecycle ─────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_full_lifecycle() {
    let backend = create_test_backend();

    // Initialize
    let init_result = backend
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    assert_eq!(init_result.server_info.as_ref().unwrap().name, "PHPantom");
    assert!(init_result.capabilities.completion_provider.is_some());
    assert!(init_result.capabilities.hover_provider.is_some());
    assert!(init_result.capabilities.definition_provider.is_some());
    assert!(init_result.capabilities.signature_help_provider.is_some());
    assert!(init_result.capabilities.references_provider.is_some());

    // Open a file and do completion
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_lifecycle.php",
        r#"<?php
class SmokeCar {
    public string $color;
    public function drive(): void {}
}
$car = new SmokeCar();
$car-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("drive")),
        "Expected `drive` in completion, got: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("color")),
        "Expected `color` in completion, got: {labels:?}"
    );

    // Shutdown
    let shutdown = backend.shutdown().await;
    assert!(shutdown.is_ok());
}

// ─── Completion smoke tests ─────────────────────────────────────────────────

#[tokio::test]
async fn smoke_completion_basic_member_access() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_basic.php",
        r#"<?php
class Logger {
    public function info(string $msg): void {}
    public function error(string $msg): void {}
    public function debug(string $msg): void {}
    private function internal(): void {}
}
$log = new Logger();
$log-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(labels.iter().any(|l| l.starts_with("info")));
    assert!(labels.iter().any(|l| l.starts_with("error")));
    assert!(labels.iter().any(|l| l.starts_with("debug")));
    // Private members should not appear from outside
    assert!(
        !labels.iter().any(|l| l.starts_with("internal")),
        "Private method should not appear: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_inheritance_chain() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_inherit.php",
        r#"<?php
class Animal {
    public function breathe(): void {}
}
class Dog extends Animal {
    public function bark(): void {}
}
class Puppy extends Dog {
    public function play(): void {}
}
$p = new Puppy();
$p-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("play")),
        "Own method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("bark")),
        "Parent method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("breathe")),
        "Grandparent method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_static_access() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_static.php",
        r#"<?php
class Config {
    public static string $appName = 'PHPantom';
    public static function get(string $key): mixed {}
    public function instanceMethod(): void {}
}
Config::<>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("get")),
        "Static method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("$appName")),
        "Static property missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_chained_methods() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_chain.php",
        r#"<?php
class QueryBuilder {
    public function where(string $col, mixed $val): self {}
    public function orderBy(string $col): self {}
    public function limit(int $n): self {}
    public function get(): array {}
}
$qb = new QueryBuilder();
$qb->where('id', 1)->orderBy('name')-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("limit")),
        "Chained method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("get")),
        "Terminal method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_docblock_var() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_docblock.php",
        r#"<?php
class Wheel {
    public function rotate(): void {}
}
class Bike {
    /** @var Wheel */
    public $frontWheel;
}
$b = new Bike();
$b->frontWheel-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("rotate")),
        "Docblock-typed property chain failed: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_interface_type_hint() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_iface.php",
        r#"<?php
interface Renderable {
    public function render(): string;
}
class View implements Renderable {
    public function render(): string {}
    public function compile(): void {}
}
function display(Renderable $r): void {
    $r-><>
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("render")),
        "Interface method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_trait_members() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_trait.php",
        r#"<?php
trait HasTimestamps {
    public function getCreatedAt(): string {}
    public function getUpdatedAt(): string {}
}
class Post {
    use HasTimestamps;
    public function getTitle(): string {}
}
$p = new Post();
$p-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("getTitle")),
        "Own method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getCreatedAt")),
        "Trait method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_enum() {
    let backend = create_test_backend();

    // Static access via :: shows cases
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_enum.php",
        r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;

    public function label(): string {}
}
Color::<>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("Red")),
        "Enum case missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("Green")),
        "Enum case missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("Blue")),
        "Enum case missing: {labels:?}"
    );

    // Instance access via $this-> inside an enum method shows instance methods
    let (uri2, line2, ch2) = open_with_cursor(
        &backend,
        "file:///smoke_enum_instance.php",
        r#"<?php
enum Color {
    case Red;
    case Green;
    case Blue;

    public function label(): string {}

    public function describe(): string {
        $this-><>
    }
}
"#,
    )
    .await;

    let instance_labels = complete_at(&backend, &uri2, line2, ch2).await;
    assert!(
        instance_labels.iter().any(|l| l.starts_with("label")),
        "Enum instance method missing: {instance_labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_generics_extends() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_generics.php",
        r#"<?php
/** @template T */
class Collection {
    /** @return T */
    public function first() {}
}
class Product {
    public function getPrice(): float {}
}
/** @extends Collection<Product> */
class ProductCollection extends Collection {}
$pc = new ProductCollection();
$pc->first()-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("getPrice")),
        "Generic resolution through @extends failed: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_foreach_typed_array() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_foreach.php",
        r#"<?php
class Item {
    public function getName(): string {}
}
class Cart {
    /** @return Item[] */
    public function getItems(): array {}
}
$cart = new Cart();
foreach ($cart->getItems() as $item) {
    $item-><>
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("getName")),
        "Foreach item type resolution failed: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_narrowing_instanceof() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_narrowing.php",
        r#"<?php
class Cat {
    public function meow(): void {}
}
class Fish {
    public function swim(): void {}
}
function handle(Cat|Fish $animal): void {
    if ($animal instanceof Cat) {
        $animal-><>
    }
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("meow")),
        "Narrowed type should show Cat methods: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_mixin() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_mixin.php",
        r#"<?php
class Abilities {
    public function fly(): void {}
    public function swim(): void {}
}
/** @mixin Abilities */
class Superhero {
    public function punch(): void {}
}
$hero = new Superhero();
$hero-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("punch")),
        "Own method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("fly")),
        "Mixin method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_virtual_methods() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_virtual.php",
        r#"<?php
/**
 * @method string getName()
 * @method void setAge(int $age)
 * @property string $email
 */
class Entity {
    public function save(): void {}
}
$e = new Entity();
$e-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("save")),
        "Real method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getName")),
        "@method virtual method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("email")),
        "@property virtual property missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_completion_this_inside_class() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_this.php",
        r#"<?php
class Service {
    private string $name;
    private function helper(): void {}
    public function run(): void {
        $this-><>
    }
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("name")),
        "Private property via $this missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("helper")),
        "Private method via $this missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("run")),
        "Public method via $this missing: {labels:?}"
    );
}

// ─── Hover smoke tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_hover_class_name() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_hover_class.php",
        r#"<?php
class Router {
    public function addRoute(string $path): void {}
}
$r = new <>Router();
"#,
    )
    .await;

    let hover = hover_at(&backend, &uri, line, ch).await;
    assert!(hover.is_some(), "Hover on class name should return content");
    let text = hover.unwrap();
    assert!(
        text.contains("Router"),
        "Hover should mention the class name, got: {text}"
    );
}

#[tokio::test]
async fn smoke_hover_method_call() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_hover_method.php",
        r#"<?php
class Calculator {
    /**
     * Add two numbers together.
     * @param int $a First number
     * @param int $b Second number
     * @return int The sum
     */
    public function add(int $a, int $b): int {}
}
$calc = new Calculator();
$calc-><>add(1, 2);
"#,
    )
    .await;

    let hover = hover_at(&backend, &uri, line, ch).await;
    assert!(
        hover.is_some(),
        "Hover on method call should return content"
    );
    let text = hover.unwrap();
    assert!(
        text.contains("add"),
        "Hover should mention the method name, got: {text}"
    );
}

#[tokio::test]
async fn smoke_hover_property() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_hover_prop.php",
        r#"<?php
class User {
    public string $name;
    public int $age;
}
$u = new User();
$u-><>name;
"#,
    )
    .await;

    let hover = hover_at(&backend, &uri, line, ch).await;
    assert!(
        hover.is_some(),
        "Hover on property access should return content"
    );
    let text = hover.unwrap();
    assert!(
        text.contains("name"),
        "Hover should mention the property, got: {text}"
    );
}

#[tokio::test]
async fn smoke_hover_variable() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_hover_var.php",
        r#"<?php
class Database {
    public function query(): void {}
}
$db = new Database();
<>$db->query();
"#,
    )
    .await;

    let hover = hover_at(&backend, &uri, line, ch).await;
    assert!(hover.is_some(), "Hover on variable should return content");
    let text = hover.unwrap();
    assert!(
        text.contains("Database"),
        "Hover should show the resolved type, got: {text}"
    );
}

#[tokio::test]
async fn smoke_hover_parenthesized_invoke_chain() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_hover_paren_invoke.php",
        r#"<?php
class HvPen { public function write(): void {} }
class HvFormatter {
    public function __invoke(): HvPen {}
}
class HvApp {
    private HvFormatter $formatter;
    public function demo(): void {
        ($this->formatter)()-><>write();
    }
}
"#,
    )
    .await;

    let hover = hover_at(&backend, &uri, line, ch).await;
    assert!(
        hover.is_some(),
        "Hover on write() after ($this->formatter)() should return content"
    );
    let text = hover.unwrap();
    assert!(
        text.contains("write"),
        "Hover should mention the method name, got: {text}"
    );
}

// ─── Go-to-definition smoke tests ──────────────────────────────────────────

#[tokio::test]
async fn smoke_definition_parenthesized_invoke_chain() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_def_paren_invoke.php",
        r#"<?php
class GtdPen { public function write(): void {} }
class GtdFormatter {
    public function __invoke(): GtdPen {}
}
class GtdApp {
    private GtdFormatter $formatter;
    public function demo(): void {
        ($this->formatter)()-><>write();
    }
}
"#,
    )
    .await;

    let locations = definition_at(&backend, &uri, line, ch).await;
    assert!(
        !locations.is_empty(),
        "GTD on write() after ($this->formatter)() should return a location"
    );
    // Should point to the write() declaration inside GtdPen (line 1)
    assert_eq!(
        locations[0].range.start.line, 1,
        "Should jump to write() declaration in GtdPen"
    );
}

#[tokio::test]
async fn smoke_definition_class_instantiation() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_def_class.php",
        r#"<?php
class Printer {
    public function print(): void {}
}
$p = new <>Printer();
"#,
    )
    .await;

    let locations = definition_at(&backend, &uri, line, ch).await;
    assert!(
        !locations.is_empty(),
        "GTD on class name should return a location"
    );
    // Should point to the class declaration line (line 1, 0-based)
    assert_eq!(
        locations[0].range.start.line, 1,
        "Should jump to class declaration"
    );
}

#[tokio::test]
async fn smoke_definition_method_call() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_def_method.php",
        r#"<?php
class Mailer {
    public function send(string $to): void {}
}
$m = new Mailer();
$m-><>send('test@example.com');
"#,
    )
    .await;

    let locations = definition_at(&backend, &uri, line, ch).await;
    assert!(
        !locations.is_empty(),
        "GTD on method call should return a location"
    );
    // Should point to the method declaration (line 2, 0-based)
    assert_eq!(
        locations[0].range.start.line, 2,
        "Should jump to method declaration"
    );
}

#[tokio::test]
async fn smoke_definition_property_access() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_def_prop.php",
        r#"<?php
class Settings {
    public bool $debug = false;
    public string $env = 'production';
}
$s = new Settings();
$s-><>debug;
"#,
    )
    .await;

    let locations = definition_at(&backend, &uri, line, ch).await;
    assert!(
        !locations.is_empty(),
        "GTD on property access should return a location"
    );
    assert_eq!(
        locations[0].range.start.line, 2,
        "Should jump to property declaration"
    );
}

#[tokio::test]
async fn smoke_definition_inherited_method() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_def_inherit.php",
        r#"<?php
class BaseRepo {
    public function find(int $id): void {}
}
class UserRepo extends BaseRepo {
    public function findByEmail(string $email): void {}
}
$repo = new UserRepo();
$repo-><>find(1);
"#,
    )
    .await;

    // Click on "find" in `$repo->find(1)` — should jump to BaseRepo::find
    let locations = definition_at(&backend, &uri, line, ch).await;
    assert!(
        !locations.is_empty(),
        "GTD on inherited method should return a location"
    );
    assert_eq!(
        locations[0].range.start.line, 2,
        "Should jump to parent class method declaration"
    );
}

// ─── Signature help smoke tests ─────────────────────────────────────────────

#[tokio::test]
async fn smoke_signature_help_basic() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_sig.php",
        r#"<?php
class Math {
    public function add(int $a, int $b): int {}
}
$m = new Math();
$m->add(<>)
"#,
    )
    .await;

    let sig = signature_at(&backend, &uri, line, ch).await;
    assert!(sig.is_some(), "Signature help should return a result");
    let sh = sig.unwrap();
    assert!(
        !sh.signatures.is_empty(),
        "Should have at least one signature"
    );
    let label = &sh.signatures[0].label;
    assert!(
        label.contains("$a") && label.contains("$b"),
        "Signature label should contain parameters, got: {label}"
    );
}

#[tokio::test]
async fn smoke_signature_help_active_parameter() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_sig_active.php",
        r#"<?php
class Formatter {
    public function format(string $text, int $width, bool $wrap): string {}
}
$f = new Formatter();
$f->format('hello', 80, <>)
"#,
    )
    .await;

    let sig = signature_at(&backend, &uri, line, ch).await;
    assert!(sig.is_some(), "Signature help should return a result");
    let sh = sig.unwrap();
    assert_eq!(
        sh.active_parameter,
        Some(2),
        "Active parameter should be 2 (third param)"
    );
}

#[tokio::test]
async fn smoke_signature_help_constructor() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_sig_ctor.php",
        r#"<?php
class Point {
    public function __construct(public float $x, public float $y) {}
}
$p = new Point(<>)
"#,
    )
    .await;

    let sig = signature_at(&backend, &uri, line, ch).await;
    assert!(
        sig.is_some(),
        "Signature help on constructor should return a result"
    );
    let sh = sig.unwrap();
    assert!(!sh.signatures.is_empty());
    let label = &sh.signatures[0].label;
    assert!(
        label.contains("$x") && label.contains("$y"),
        "Constructor signature should show parameters, got: {label}"
    );
}

#[tokio::test]
async fn smoke_signature_help_static_method() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_sig_static.php",
        r#"<?php
class Factory {
    public static function create(string $name, array $options): self {}
}
Factory::create(<>)
"#,
    )
    .await;

    let sig = signature_at(&backend, &uri, line, ch).await;
    assert!(
        sig.is_some(),
        "Signature help on static method should return a result"
    );
    let sh = sig.unwrap();
    assert!(!sh.signatures.is_empty());
}

#[tokio::test]
async fn smoke_signature_help_parenthesized_invoke_chain() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_sig_paren_invoke.php",
        r#"<?php
class SigPen { public function write(string $text): void {} }
class SigFormatter {
    public function __invoke(): SigPen {}
}
class SigApp {
    private SigFormatter $formatter;
    public function demo(): void {
        ($this->formatter)()->write(<>);
    }
}
"#,
    )
    .await;

    let sig = signature_at(&backend, &uri, line, ch).await;
    assert!(
        sig.is_some(),
        "Signature help on write() after ($this->formatter)() should return a result"
    );
    let sh = sig.unwrap();
    assert!(!sh.signatures.is_empty());
    assert!(
        sh.signatures[0].label.contains("$text"),
        "Signature should show $text parameter, got: {}",
        sh.signatures[0].label
    );
}

// ─── Cross-file smoke tests ────────────────────────────────────────────────

#[tokio::test]
async fn smoke_cross_file_completion() {
    let (backend, _tmp) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Repository.php",
                r#"<?php
namespace App;
class Repository {
    public function findAll(): array {}
    public function save(object $entity): void {}
}
"#,
            ),
            (
                "src/Service.php",
                r#"<?php
namespace App;
class Service {
    public function run(Repository $repo): void {
        $repo->
    }
}
"#,
            ),
        ],
    );

    let uri =
        Url::from_file_path(_tmp.path().join("src/Service.php").canonicalize().unwrap()).unwrap();

    let content = std::fs::read_to_string(_tmp.path().join("src/Service.php")).unwrap();
    open_php(&backend, &uri, &content).await;

    // Cursor right after `$repo->` on line 4 (0-based), character 15
    let labels = complete_at(&backend, &uri, 4, 15).await;
    assert!(
        labels.iter().any(|l| l.starts_with("findAll")),
        "Cross-file method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("save")),
        "Cross-file method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_cross_file_definition() {
    let (backend, _tmp) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[
            (
                "src/Model.php",
                r#"<?php
namespace App;
class Model {
    public function toArray(): array {}
}
"#,
            ),
            (
                "src/Controller.php",
                r#"<?php
namespace App;
class Controller {
    public function index(Model $model): void {
        $model->toArray();
    }
}
"#,
            ),
        ],
    );

    let uri = Url::from_file_path(
        _tmp.path()
            .join("src/Controller.php")
            .canonicalize()
            .unwrap(),
    )
    .unwrap();

    let content = std::fs::read_to_string(_tmp.path().join("src/Controller.php")).unwrap();
    open_php(&backend, &uri, &content).await;

    // Click on "toArray" in `$model->toArray()` — line 4, around character 17
    let locations = definition_at(&backend, &uri, 4, 17).await;
    assert!(
        !locations.is_empty(),
        "GTD on cross-file method should return a location"
    );
}

// ─── Complex scenario smoke tests ──────────────────────────────────────────

#[tokio::test]
async fn smoke_complex_builder_pattern() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_builder.php",
        r#"<?php
class EmailBuilder {
    public function to(string $addr): self {}
    public function from(string $addr): self {}
    public function subject(string $s): self {}
    public function body(string $b): self {}
    public function send(): bool {}
}
$email = new EmailBuilder();
$email->to('a@b.com')->from('x@y.com')->subject('Hi')-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("body")),
        "Builder chain should resolve: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("send")),
        "Builder chain should resolve: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_complex_generic_collection_foreach() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_generic_foreach.php",
        r#"<?php
/** @template T */
class TypedList {
    /** @return T[] */
    public function all(): array {}
}
class Order {
    public function getTotal(): float {}
    public function getStatus(): string {}
}
/** @extends TypedList<Order> */
class OrderList extends TypedList {}

$orders = new OrderList();
foreach ($orders->all() as $order) {
    $order-><>
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("getTotal")),
        "Generic collection foreach failed: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getStatus")),
        "Generic collection foreach failed: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_complex_guard_clause_narrowing() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_guard.php",
        r#"<?php
class Success {
    public function getData(): array {}
}
class Error {
    public function getMessage(): string {}
}
function handle(Success|Error $result): void {
    if ($result instanceof Error) {
        return;
    }
    // $result should be narrowed to Success here
    $result-><>
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("getData")),
        "Guard clause narrowing should show Success methods: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_complex_multiple_files_open() {
    let backend = create_test_backend();

    // Open the dependency file first (no cursor)
    open_file(
        &backend,
        "file:///smoke_multi_a.php",
        r#"<?php
class Engine {
    public function start(): void {}
    public function stop(): void {}
    public function getHorsepower(): int {}
}
"#,
    )
    .await;

    // Open the consumer file with cursor
    let (uri_b, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_multi_b.php",
        r#"<?php
class Vehicle {
    public Engine $engine;
    public function drive(): void {
        $this->engine-><>
    }
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri_b, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("start")),
        "Cross-file type hint resolution failed: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("getHorsepower")),
        "Cross-file type hint resolution failed: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_complex_conditional_return_type() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_conditional.php",
        r#"<?php
class Box {
    public function open(): string {}
}
class Container {
    /**
     * @template T of object
     * @param class-string<T> $class
     * @return T
     */
    public function make(string $class) {}
}
$c = new Container();
$c->make(Box::class)-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("open")),
        "class-string<T> generic resolution failed: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_complex_array_shape() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_shape.php",
        r#"<?php
class Validator {
    public function validate(): void {}
}
class Api {
    /** @return array{validator: Validator, success: bool} */
    public function process(): array {}
}
$api = new Api();
$result = $api->process();
$result['validator']-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("validate")),
        "Array shape value type resolution failed: {labels:?}"
    );
}

// ─── Regression-style smoke tests ──────────────────────────────────────────

#[tokio::test]
async fn smoke_regression_null_safe_chain() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_nullsafe.php",
        r#"<?php
class Address {
    public function getCity(): string {}
    public function getZip(): string {}
}
class Person {
    public function getAddress(): ?Address {}
}
$p = new Person();
$p->getAddress()?-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("getCity")),
        "Null-safe chain should resolve: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_regression_parent_constructor() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_parent_ctor.php",
        r#"<?php
class BaseController {
    public function __construct(private string $name) {}
    public function redirect(): void {}
}
class PageController extends BaseController {
    public function show(): void {
        parent::<>
    }
}
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("__construct")),
        "parent:: should show constructor: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("redirect")),
        "parent:: should show parent methods: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_regression_abstract_class() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_abstract.php",
        r#"<?php
abstract class Shape {
    abstract public function area(): float;
    public function describe(): string {}
}
class Circle extends Shape {
    public function area(): float {}
    public function radius(): float {}
}
$c = new Circle();
$c-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("area")),
        "Overridden abstract method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("describe")),
        "Inherited concrete method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("radius")),
        "Own method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_regression_multiple_traits() {
    let backend = create_test_backend();
    let (uri, line, ch) = open_with_cursor(
        &backend,
        "file:///smoke_multi_trait.php",
        r#"<?php
trait Loggable {
    public function log(string $msg): void {}
}
trait Cacheable {
    public function cache(): void {}
}
class Service {
    use Loggable, Cacheable;
    public function execute(): void {}
}
$s = new Service();
$s-><>
"#,
    )
    .await;

    let labels = complete_at(&backend, &uri, line, ch).await;
    assert!(
        labels.iter().any(|l| l.starts_with("log")),
        "First trait method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("cache")),
        "Second trait method missing: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("execute")),
        "Own method missing: {labels:?}"
    );
}

#[tokio::test]
async fn smoke_regression_did_change_updates_completion() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///smoke_change.php").unwrap();

    // Open with one method
    open_php(
        &backend,
        &uri,
        r#"<?php
class Evolving {
    public function alpha(): void {}
}
$e = new Evolving();
$e->
"#,
    )
    .await;

    // Cursor right after `$e->` on line 5, character 4
    let labels = complete_at(&backend, &uri, 5, 4).await;
    assert!(labels.iter().any(|l| l.starts_with("alpha")));
    assert!(!labels.iter().any(|l| l.starts_with("beta")));

    // Change to add a second method
    let change_params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: 2,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: r#"<?php
class Evolving {
    public function alpha(): void {}
    public function beta(): void {}
}
$e = new Evolving();
$e->
"#
            .to_string(),
        }],
    };
    backend.did_change(change_params).await;

    // Now `$e->` is on line 6
    let labels = complete_at(&backend, &uri, 6, 4).await;
    assert!(
        labels.iter().any(|l| l.starts_with("alpha")),
        "Original method should still be present: {labels:?}"
    );
    assert!(
        labels.iter().any(|l| l.starts_with("beta")),
        "New method should appear after change: {labels:?}"
    );
}
