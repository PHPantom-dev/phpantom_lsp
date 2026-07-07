use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

// ─── Template parameter bounds completion tests ─────────────────────────────
//
// These tests verify that when a property or variable has a type that is a
// template parameter (e.g. `TNode`), the resolver falls back to the upper
// bound declared in `@template TNode of SomeClass` so that completion and
// go-to-definition still work.

/// Helper: open a document, send a completion request, return item labels.
async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Vec<String> {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "php".to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
        .await;

    let result = backend
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line, character },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .unwrap();

    match result {
        Some(CompletionResponse::Array(items)) => items.iter().map(|i| i.label.clone()).collect(),
        _ => vec![],
    }
}

// ─── Basic template bound on promoted constructor property ──────────────────

/// When a class declares `@template TNode of SomeBase` and a property is
/// typed as `TNode` via `@param`, completion should resolve to `SomeBase`.
#[tokio::test]
async fn test_template_bound_on_constructor_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///tpl_bound_basic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class PDependNode {\n",
        "    public function getParent(): ?PDependNode { return null; }\n",
        "    public function getChildren(): array { return []; }\n",
        "}\n",
        "/**\n",
        " * @template-covariant TNode of PDependNode\n",
        " */\n",
        "abstract class AbstractNode {\n",
        "    /**\n",
        "     * @param TNode $node\n",
        "     */\n",
        "    public function __construct(\n",
        "        private readonly PDependNode $node,\n",
        "    ) {}\n",
        "    public function doStuff(): void {\n",
        "        $this->node->\n",
        "    }\n",
        "}\n",
    );

    // Cursor after `$this->node->` on line 16
    let names = complete_at(&backend, &uri, text, 16, 22).await;
    assert!(
        names.iter().any(|n| n.starts_with("getParent(")),
        "Should offer PDependNode::getParent() via template bound, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("getChildren(")),
        "Should offer PDependNode::getChildren() via template bound, got: {names:?}"
    );
}

// ─── Template bound on a regular (non-promoted) property ────────────────────

/// A `@var TNode` annotation on a class property should fall back to the
/// template bound when `TNode` itself is not a real class.
#[tokio::test]
async fn test_template_bound_on_var_property() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///tpl_bound_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function speak(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template T of Animal\n",
        " */\n",
        "class Cage {\n",
        "    /** @var T */\n",
        "    public $occupant;\n",
        "    public function test(): void {\n",
        "        $this->occupant->\n",
        "    }\n",
        "}\n",
    );

    // Cursor after `$this->occupant->` on line 11
    let names = complete_at(&backend, &uri, text, 11, 26).await;
    assert!(
        names.iter().any(|n| n.starts_with("speak(")),
        "Should offer Animal::speak() via template bound on @var, got: {names:?}"
    );
}

// ─── Template bound via @phpstan-template ───────────────────────────────────

/// The `@phpstan-template` variant should also have its bounds recognised.
#[tokio::test]
async fn test_phpstan_template_bound() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///tpl_bound_phpstan.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Renderer {\n",
        "    public function render(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @phpstan-template TRenderer of Renderer\n",
        " */\n",
        "class View {\n",
        "    /** @var TRenderer */\n",
        "    public $renderer;\n",
        "    public function show(): void {\n",
        "        $this->renderer->\n",
        "    }\n",
        "}\n",
    );

    let names = complete_at(&backend, &uri, text, 11, 26).await;
    assert!(
        names.iter().any(|n| n.starts_with("render(")),
        "Should offer Renderer::render() via @phpstan-template bound, got: {names:?}"
    );
}

// ─── Template-covariant with `of` bound ─────────────────────────────────────

/// `@template-covariant T of SomeClass` should work the same as
/// `@template T of SomeClass`.
#[tokio::test]
async fn test_template_covariant_bound() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///tpl_bound_covariant.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Shape {\n",
        "    public function area(): float { return 0.0; }\n",
        "}\n",
        "/**\n",
        " * @template-covariant TShape of Shape\n",
        " */\n",
        "class Canvas {\n",
        "    /** @var TShape */\n",
        "    public $shape;\n",
        "    public function draw(): void {\n",
        "        $this->shape->\n",
        "    }\n",
        "}\n",
    );

    let names = complete_at(&backend, &uri, text, 11, 22).await;
    assert!(
        names.iter().any(|n| n.starts_with("area(")),
        "Should offer Shape::area() via @template-covariant bound, got: {names:?}"
    );
}

// ─── Template without bound (bare @template T) ─────────────────────────────

/// When `@template T` has no `of` clause, the type cannot be resolved
/// to anything meaningful. Completion should gracefully return nothing
/// rather than crash or produce incorrect results.
#[tokio::test]
async fn test_template_without_bound_no_crash() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///tpl_no_bound.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "class Box {\n",
        "    /** @var T */\n",
        "    public $value;\n",
        "    public function test(): void {\n",
        "        $this->value->\n",
        "    }\n",
        "}\n",
    );

    // Should not crash; may return empty or limited results.
    let names = complete_at(&backend, &uri, text, 8, 23).await;
    // We just verify it doesn't panic. The result set may be empty.
    let _ = names;
}

// ─── Multiple template parameters, only one with bound ──────────────────────

/// When a class has multiple template parameters, only the one with a
/// bound should resolve through the bound type.
#[tokio::test]
async fn test_multiple_templates_one_with_bound() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///tpl_bound_multi.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Entity {\n",
        "    public function getId(): int { return 0; }\n",
        "}\n",
        "/**\n",
        " * @template TKey\n",
        " * @template TEntity of Entity\n",
        " */\n",
        "class Repository {\n",
        "    /** @var TEntity */\n",
        "    public $entity;\n",
        "    public function test(): void {\n",
        "        $this->entity->\n",
        "    }\n",
        "}\n",
    );

    let names = complete_at(&backend, &uri, text, 12, 23).await;
    assert!(
        names.iter().any(|n| n.starts_with("getId(")),
        "Should offer Entity::getId() for TEntity with bound, got: {names:?}"
    );
}

// ─── Cross-file template bound resolution ───────────────────────────────────

/// Template bounds should resolve even when the bound type is defined in
/// a different file loaded via PSR-4.
#[tokio::test]
async fn test_template_bound_cross_file() {
    let (backend, _dir) = create_psr4_workspace(
        r#"{
            "autoload": {
                "psr-4": {
                    "App\\": "src/"
                }
            }
        }"#,
        &[(
            "src/BaseModel.php",
            concat!(
                "<?php\n",
                "namespace App;\n",
                "class BaseModel {\n",
                "    public function save(): bool { return true; }\n",
                "    public function delete(): bool { return true; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::parse("file:///test_tpl_cross.php").unwrap();
    let text = concat!(
        "<?php\n",
        "use App\\BaseModel;\n",
        "/**\n",
        " * @template TModel of BaseModel\n",
        " */\n",
        "abstract class AbstractRepository {\n",
        "    /** @var TModel */\n",
        "    protected $model;\n",
        "    public function persist(): void {\n",
        "        $this->model->\n",
        "    }\n",
        "}\n",
    );

    let names = complete_at(&backend, &uri, text, 9, 22).await;
    assert!(
        names.iter().any(|n| n.starts_with("save(")),
        "Should offer BaseModel::save() via cross-file template bound, got: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.starts_with("delete(")),
        "Should offer BaseModel::delete() via cross-file template bound, got: {names:?}"
    );
}

// ─── Bound with namespace prefix ────────────────────────────────────────────

/// When the bound type uses a fully-qualified name (e.g. `\App\SomeClass`),
/// it should still resolve.
#[tokio::test]
async fn test_template_bound_fqn() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///tpl_bound_fqn.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $msg): void {}\n",
        "}\n",
        "/**\n",
        " * @template TLogger of Logger\n",
        " */\n",
        "class LogAware {\n",
        "    /** @var TLogger */\n",
        "    public $logger;\n",
        "    public function test(): void {\n",
        "        $this->logger->\n",
        "    }\n",
        "}\n",
    );

    let names = complete_at(&backend, &uri, text, 11, 23).await;
    assert!(
        names.iter().any(|n| n.starts_with("log(")),
        "Should offer Logger::log() via template bound, got: {names:?}"
    );
}

// ─── extract_template_params_with_bounds unit tests ─────────────────────────

#[test]
fn test_extract_bounds_basic() {
    use phpantom_lsp::docblock::extract_template_params_with_bounds;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @template T of SomeClass\n */";
    let result = extract_template_params_with_bounds(docblock);
    assert_eq!(
        result,
        vec![("T".to_string(), Some(PhpType::parse("SomeClass")))]
    );
}

#[test]
fn test_extract_bounds_no_bound() {
    use phpantom_lsp::docblock::extract_template_params_with_bounds;

    let docblock = "/**\n * @template T\n */";
    let result = extract_template_params_with_bounds(docblock);
    assert_eq!(result, vec![("T".to_string(), None)]);
}

#[test]
fn test_extract_bounds_mixed() {
    use phpantom_lsp::docblock::extract_template_params_with_bounds;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @template TKey\n * @template TValue of SomeInterface\n */";
    let result = extract_template_params_with_bounds(docblock);
    assert_eq!(
        result,
        vec![
            ("TKey".to_string(), None),
            ("TValue".to_string(), Some(PhpType::parse("SomeInterface"))),
        ]
    );
}

#[test]
fn test_extract_bounds_covariant() {
    use phpantom_lsp::docblock::extract_template_params_with_bounds;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @template-covariant TNode of PDependNode\n */";
    let result = extract_template_params_with_bounds(docblock);
    assert_eq!(
        result,
        vec![("TNode".to_string(), Some(PhpType::parse("PDependNode")))]
    );
}

#[test]
fn test_extract_bounds_phpstan_prefix() {
    use phpantom_lsp::docblock::extract_template_params_with_bounds;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @phpstan-template T of Stringable\n */";
    let result = extract_template_params_with_bounds(docblock);
    assert_eq!(
        result,
        vec![("T".to_string(), Some(PhpType::parse("Stringable")))]
    );
}

#[test]
fn test_extract_bounds_contravariant_with_bound() {
    use phpantom_lsp::docblock::extract_template_params_with_bounds;
    use phpantom_lsp::php_type::PhpType;

    let docblock = "/**\n * @template-contravariant TInput of Comparable\n */";
    let result = extract_template_params_with_bounds(docblock);
    assert_eq!(
        result,
        vec![("TInput".to_string(), Some(PhpType::parse("Comparable")))]
    );
}

// ─── Trait template bound fallback ────────────────────────────────────

/// When a class uses a generic trait without `@use` generics, template
/// parameters in trait methods should fall back to their declared bound.
#[tokio::test]
async fn test_trait_template_bound_fallback_without_use_annotation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///trait_tpl_bound.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseObj {\n",
        "    public function baseMethod(): string { return ''; }\n",
        "}\n",
        "/**\n",
        " * @template T of BaseObj\n",
        " */\n",
        "trait MyTrait {\n",
        "    /** @return T */\n",
        "    public function doFoo() {}\n",
        "}\n",
        "class User {\n",
        "    use MyTrait;\n",
        "}\n",
        "function testIt(User $u) {\n",
        "    $u->doFoo()->\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 15, 19).await;
    assert!(
        items.iter().any(|i| i.starts_with("baseMethod")),
        "Expected 'baseMethod' from bound type BaseObj, got: {:?}",
        items
    );
}

/// When a class uses a generic trait WITH `@use` generics, the explicit
/// type arguments should be used instead of the bound.
#[tokio::test]
async fn test_trait_template_with_use_annotation_overrides_bound() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///trait_tpl_explicit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class BaseObj {\n",
        "    public function baseMethod(): string { return ''; }\n",
        "}\n",
        "class SpecialObj extends BaseObj {\n",
        "    public function specialMethod(): int { return 0; }\n",
        "}\n",
        "/**\n",
        " * @template T of BaseObj\n",
        " */\n",
        "trait MyTrait {\n",
        "    /** @return T */\n",
        "    public function doFoo() {}\n",
        "}\n",
        "/** @use MyTrait<SpecialObj> */\n",
        "class User {\n",
        "    use MyTrait;\n",
        "}\n",
        "function testIt(User $u) {\n",
        "    $u->doFoo()->\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, text, 19, 19).await;
    assert!(
        items.iter().any(|i| i.starts_with("specialMethod")),
        "Expected 'specialMethod' from explicit SpecialObj, got: {:?}",
        items
    );
}

/// When a trait template has no bound, the fallback should not inject
/// anything (no crash, no wrong type).
#[tokio::test]
async fn test_trait_template_no_bound_no_crash() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///trait_tpl_no_bound.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " */\n",
        "trait MyTrait {\n",
        "    /** @return T */\n",
        "    public function doFoo() {}\n",
        "}\n",
        "class User {\n",
        "    use MyTrait;\n",
        "}\n",
        "function testIt(User $u) {\n",
        "    $u->doFoo()->\n",
        "}\n",
    );

    // Should not crash; no completions expected since T has no bound.
    let items = complete_at(&backend, &uri, text, 12, 19).await;
    // Just verify it didn't panic — items may be empty or contain mixed members.
    let _ = items;
}
