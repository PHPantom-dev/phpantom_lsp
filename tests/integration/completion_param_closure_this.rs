use crate::common::{create_psr4_workspace, create_test_backend};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a document and trigger completion at the given line/column.
async fn complete_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    src: &str,
    line: u32,
    character: u32,
) -> Vec<CompletionItem> {
    let open_params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "php".to_string(),
            version: 1,
            text: src.to_string(),
        },
    };
    backend.did_open(open_params).await;

    let completion_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position { line, character },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
        context: None,
    };

    match backend.completion(completion_params).await.unwrap() {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    }
}

fn method_names(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::METHOD))
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect()
}

fn property_names(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|i| i.kind == Some(CompletionItemKind::PROPERTY))
        .map(|i| i.filter_text.as_deref().unwrap_or(&i.label))
        .collect()
}

// ─── @param-closure-this: instance method call ──────────────────────────────

/// When a method parameter has `@param-closure-this Route $callback`,
/// `$this->` inside a closure passed for that parameter should resolve
/// to `Route`, not the lexically enclosing class.
#[tokio::test]
async fn test_param_closure_this_instance_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_tag.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Route {\n",
        "    public function middleware(string $m): self { return $this; }\n",
        "    public function prefix(string $p): self { return $this; }\n",
        "}\n",
        "class Router {\n",
        "    /**\n",
        "     * @param-closure-this Route $callback\n",
        "     */\n",
        "    public function group(\\Closure $callback): void {}\n",
        "}\n",
        "class AppRoutes {\n",
        "    public function register(): void {\n",
        "        $router = new Router();\n",
        "        $router->group(function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 15: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 15, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"middleware"),
        "Expected 'middleware' from @param-closure-this Route, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"prefix"),
        "Expected 'prefix' from @param-closure-this Route, got: {:?}",
        names,
    );
}

// ─── @param-closure-this with `$this` as the type ───────────────────────────

/// `@param-closure-this $this $callback` means `$this` inside the closure
/// refers to the declaring class (the class that owns the method).
#[tokio::test]
async fn test_param_closure_this_dollar_this_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_self.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class CacheManager {\n",
        "    public function getDefaultDriver(): string { return ''; }\n",
        "    /**\n",
        "     * @param string $driver\n",
        "     * @param \\Closure $callback\n",
        "     * @param-closure-this $this $callback\n",
        "     * @return $this\n",
        "     */\n",
        "    public function extend(string $driver, \\Closure $callback): self { return $this; }\n",
        "}\n",
        "class App {\n",
        "    public function boot(): void {\n",
        "        $mgr = new CacheManager();\n",
        "        $mgr->extend('redis', function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 15: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 15, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getDefaultDriver"),
        "Expected 'getDefaultDriver' from @param-closure-this $this (CacheManager), got: {:?}",
        names,
    );
    assert!(
        names.contains(&"extend"),
        "Expected 'extend' from @param-closure-this $this (CacheManager), got: {:?}",
        names,
    );
}

// ─── @param-closure-this with `static` as the type ──────────────────────────

/// `@param-closure-this static $macro` means `$this` inside the closure
/// refers to the declaring class.
#[tokio::test]
async fn test_param_closure_this_static_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_static.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Macroable {\n",
        "    public function getMacros(): array { return []; }\n",
        "    /**\n",
        "     * @param string $name\n",
        "     * @param callable $macro\n",
        "     * @param-closure-this static $macro\n",
        "     */\n",
        "    public static function macro(string $name, callable $macro): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        Macroable::macro('test', function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 13, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"getMacros"),
        "Expected 'getMacros' from @param-closure-this static (Macroable), got: {:?}",
        names,
    );
}

// ─── @param-closure-this on a standalone function ───────────────────────────

/// `@param-closure-this` works on standalone functions too, not just methods.
#[tokio::test]
async fn test_param_closure_this_standalone_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_func.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class TestCase {\n",
        "    public function assertTrue(bool $v): void {}\n",
        "    public function assertFalse(bool $v): void {}\n",
        "}\n",
        "/**\n",
        " * @param-closure-this TestCase $callback\n",
        " */\n",
        "function test(string $name, \\Closure $callback): void {}\n",
        "class Runner {\n",
        "    public function go(): void {\n",
        "        test('example', function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 12: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 12, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"assertTrue"),
        "Expected 'assertTrue' from @param-closure-this TestCase, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"assertFalse"),
        "Expected 'assertFalse' from @param-closure-this TestCase, got: {:?}",
        names,
    );
}

// ─── @param-closure-this does not leak outside the closure ──────────────────

/// `$this` outside the closure should still resolve to the lexically
/// enclosing class, not the @param-closure-this type.
#[tokio::test]
async fn test_param_closure_this_does_not_leak() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_no_leak.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Route {\n",
        "    public function middleware(string $m): self { return $this; }\n",
        "}\n",
        "class Router {\n",
        "    /**\n",
        "     * @param-closure-this Route $callback\n",
        "     */\n",
        "    public function group(\\Closure $callback): void {}\n",
        "}\n",
        "class AppRoutes {\n",
        "    public function ownMethod(): string { return ''; }\n",
        "    public function register(): void {\n",
        "        $router = new Router();\n",
        "        $this->\n",
        "        $router->group(function () {\n",
        "            // inside closure: $this is Route\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 14: `        $this->` — cursor OUTSIDE the closure
    let items = complete_at(&backend, &uri, src, 14, 15).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"ownMethod"),
        "Expected 'ownMethod' from lexical class AppRoutes, got: {:?}",
        names,
    );
    assert!(
        !names.contains(&"middleware"),
        "Should NOT see Route::middleware outside the closure, got: {:?}",
        names,
    );
}

// ─── @param-closure-this with property access ───────────────────────────────

/// `$this->prop` inside a closure with @param-closure-this should
/// resolve against the override type's properties.
#[tokio::test]
async fn test_param_closure_this_property_access() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_prop.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Route {\n",
        "    /** @var string */\n",
        "    public string $uri = '';\n",
        "}\n",
        "class Router {\n",
        "    /**\n",
        "     * @param-closure-this Route $callback\n",
        "     */\n",
        "    public function group(\\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $r = new Router();\n",
        "        $r->group(function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 15: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 15, 19).await;
    let props = property_names(&items);
    assert!(
        props.contains(&"uri"),
        "Expected 'uri' property from @param-closure-this Route, got: {:?}",
        props,
    );
}

// ─── self:: / static:: inside a rebound closure ─────────────────────────────

/// Inside a closure rebound via `@param-closure-this` (e.g. a macro
/// registration), the runtime binds the target class as the closure's
/// scope, so `self::` refers to the macro target, not the class that
/// lexically encloses the registration.
#[tokio::test]
async fn test_param_closure_this_self_keyword() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_self_kw.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Macroable {\n",
        "    public static function make(): static { return new static(); }\n",
        "    /**\n",
        "     * @param string $name\n",
        "     * @param callable $macro\n",
        "     * @param-closure-this static $macro\n",
        "     */\n",
        "    public static function macro(string $name, callable $macro): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        Macroable::macro('test', function () {\n",
        "            self::\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `            self::` — cursor after `::`
    let items = complete_at(&backend, &uri, src, 13, 18).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"make"),
        "Expected 'make' from the macro target (Macroable) via self::, got: {:?}",
        names,
    );
}

/// Same as above for the `static::` keyword.
#[tokio::test]
async fn test_param_closure_this_static_keyword() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_static_kw.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Macroable {\n",
        "    public static function make(): static { return new static(); }\n",
        "    /**\n",
        "     * @param string $name\n",
        "     * @param callable $macro\n",
        "     * @param-closure-this static $macro\n",
        "     */\n",
        "    public static function macro(string $name, callable $macro): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        Macroable::macro('test', function () {\n",
        "            static::\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `            static::` — cursor after `::`
    let items = complete_at(&backend, &uri, src, 13, 20).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"make"),
        "Expected 'make' from the macro target (Macroable) via static::, got: {:?}",
        names,
    );
}

/// `self::` outside the closure must still resolve to the lexically
/// enclosing class, not the @param-closure-this type.
#[tokio::test]
async fn test_param_closure_this_self_keyword_does_not_leak() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_self_kw_no_leak.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Macroable {\n",
        "    public static function make(): static { return new static(); }\n",
        "    /**\n",
        "     * @param-closure-this static $macro\n",
        "     */\n",
        "    public static function macro(string $name, callable $macro): void {}\n",
        "}\n",
        "class App {\n",
        "    public static function ownHelper(): string { return ''; }\n",
        "    public function run(): void {\n",
        "        Macroable::macro('test', function () {\n",
        "        });\n",
        "        self::\n",
        "    }\n",
        "}\n",
    );

    // Line 13: `        self::` — cursor OUTSIDE the closure
    let items = complete_at(&backend, &uri, src, 13, 14).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"ownHelper"),
        "Expected 'ownHelper' from lexical class App, got: {:?}",
        names,
    );
    assert!(
        !names.contains(&"make"),
        "Should NOT see Macroable::make outside the closure, got: {:?}",
        names,
    );
}

// ─── @param-closure-this with FQN type ──────────────────────────────────────

/// `@param-closure-this \App\Route $callback` with a leading backslash
/// should resolve correctly.
#[tokio::test]
async fn test_param_closure_this_fqn_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_fqn.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Route {\n",
        "    public function middleware(string $m): self { return $this; }\n",
        "}\n",
        "class Router {\n",
        "    /**\n",
        "     * @param-closure-this \\Route $callback\n",
        "     */\n",
        "    public function group(\\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $r = new Router();\n",
        "        $r->group(function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 14: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 14, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"middleware"),
        "Expected 'middleware' from @param-closure-this \\Route, got: {:?}",
        names,
    );
}

// ─── @param-closure-this cross-file via PSR-4 ───────────────────────────────

/// The @param-closure-this type should resolve across files using the
/// class loader (PSR-4 autoloading).
#[tokio::test]
async fn test_param_closure_this_cross_file() {
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#,
        &[
            (
                "src/Route.php",
                "<?php\nnamespace App;\nclass Route {\n    public function middleware(string $m): self { return $this; }\n    public function prefix(string $p): self { return $this; }\n}\n",
            ),
            (
                "src/Router.php",
                concat!(
                    "<?php\nnamespace App;\n",
                    "class Router {\n",
                    "    /**\n",
                    "     * @param-closure-this \\App\\Route $callback\n",
                    "     */\n",
                    "    public function group(\\Closure $callback): void {}\n",
                    "}\n",
                ),
            ),
        ],
    );

    let uri = Url::from_file_path(dir.path().join("src/AppRoutes.php")).unwrap();

    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "class AppRoutes {\n",
        "    public function register(): void {\n",
        "        $router = new Router();\n",
        "        $router->group(function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 6, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"middleware"),
        "Expected 'middleware' from cross-file @param-closure-this Route, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"prefix"),
        "Expected 'prefix' from cross-file @param-closure-this Route, got: {:?}",
        names,
    );
}

// ─── @param-closure-this resolved against the declaring file ────────────────

/// The tag names its class the way the rest of the declaring file's
/// docblocks do, so it has to be resolved against *that* file's imports.
/// The caller never spells the class itself, so its own use table has
/// nothing to offer and the closure's `$this` would fall back to the
/// enclosing class.
#[tokio::test]
async fn param_closure_this_resolves_against_the_declaring_file_not_the_caller() {
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload": {"psr-4": {"App\\": "src/"}}}"#,
        &[(
            "src/Support/Router.php",
            concat!(
                "<?php\nnamespace App\\Support;\n",
                "class Router {\n",
                "    /**\n",
                "     * @param-closure-this Route $callback\n",
                "     */\n",
                "    public function group(\\Closure $callback): void {}\n",
                "}\n",
                "class Route {\n",
                "    public function middleware(string $m): self { return $this; }\n",
                "}\n",
            ),
        )],
    );

    let uri = Url::from_file_path(dir.path().join("src/Group.php")).unwrap();

    // The caller imports only `Router`, and has a `Route`-less namespace
    // of its own, so a tag resolved from here would find nothing.
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "use App\\Support\\Router;\n",
        "class Group {\n",
        "    public function run(): void {\n",
        "        (new Router())->group(function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    let items = complete_at(&backend, &uri, src, 6, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"middleware"),
        "Expected 'middleware' from App\\Support\\Route, got: {:?}",
        names,
    );
}

// ─── @param-closure-this second parameter ───────────────────────────────────

/// When @param-closure-this targets the second parameter, only closures
/// passed as the second argument should be affected.
#[tokio::test]
async fn test_param_closure_this_second_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_second.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Route {\n",
        "    public function middleware(string $m): self { return $this; }\n",
        "}\n",
        "class Router {\n",
        "    /**\n",
        "     * @param string $prefix\n",
        "     * @param \\Closure $callback\n",
        "     * @param-closure-this Route $callback\n",
        "     */\n",
        "    public function group(string $prefix, \\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $r = new Router();\n",
        "        $r->group('/api', function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 16: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 16, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"middleware"),
        "Expected 'middleware' from @param-closure-this Route on second param, got: {:?}",
        names,
    );
}

// ─── @param-closure-this with chained method call ───────────────────────────

/// `$this->method()` inside a closure with @param-closure-this should
/// resolve through the override type.
#[tokio::test]
async fn test_param_closure_this_method_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_chain.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Route {\n",
        "    public function middleware(string $m): self { return $this; }\n",
        "    public function prefix(string $p): self { return $this; }\n",
        "}\n",
        "class Router {\n",
        "    /**\n",
        "     * @param-closure-this Route $callback\n",
        "     */\n",
        "    public function group(\\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function run(): void {\n",
        "        $r = new Router();\n",
        "        $r->group(function () {\n",
        "            $this->middleware('auth')->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 15: `            $this->middleware('auth')->` — cursor after second `->`
    let items = complete_at(&backend, &uri, src, 15, 42).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"prefix"),
        "Expected 'prefix' from chained call on @param-closure-this Route, got: {:?}",
        names,
    );
}

// ─── @param-closure-this in nested closures ─────────────────────────────────

/// A closure inside a closure, where both call sites declare
/// `@param-closure-this`: the innermost annotation wins.  The inner call's
/// own receiver (`$this`) is itself resolved through the outer override.
#[tokio::test]
async fn test_param_closure_this_nested_closures() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_nested.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Inner {\n",
        "    public function innerOnly(): void {}\n",
        "}\n",
        "class Outer {\n",
        "    /**\n",
        "     * @param-closure-this Inner $callback\n",
        "     */\n",
        "    public function withInner(\\Closure $callback): void {}\n",
        "    public function outerOnly(): void {}\n",
        "}\n",
        "class Host {\n",
        "    /**\n",
        "     * @param-closure-this Outer $callback\n",
        "     */\n",
        "    public function withOuter(\\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function boot(): void {\n",
        "        $host = new Host();\n",
        "        $host->withOuter(function () {\n",
        "            $this->withInner(function () {\n",
        "                $this->\n",
        "            });\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 22: `                $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 22, 23).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"innerOnly"),
        "Expected 'innerOnly' from the inner @param-closure-this Inner, got: {:?}",
        names,
    );
    assert!(
        !names.contains(&"outerOnly"),
        "Inner closure should not see Outer's members, got: {:?}",
        names,
    );
}

/// The outer closure body still resolves `$this` to the outer override
/// when the cursor sits outside the nested closure.
#[tokio::test]
async fn test_param_closure_this_outer_body_of_nested_closures() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_nested_outer.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Inner {\n",
        "    public function innerOnly(): void {}\n",
        "}\n",
        "class Outer {\n",
        "    /**\n",
        "     * @param-closure-this Inner $callback\n",
        "     */\n",
        "    public function withInner(\\Closure $callback): void {}\n",
        "    public function outerOnly(): void {}\n",
        "}\n",
        "class Host {\n",
        "    /**\n",
        "     * @param-closure-this Outer $callback\n",
        "     */\n",
        "    public function withOuter(\\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function boot(): void {\n",
        "        $host = new Host();\n",
        "        $host->withOuter(function () {\n",
        "            $this->withInner(function () {});\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 22: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 22, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"outerOnly"),
        "Expected 'outerOnly' from the outer @param-closure-this Outer, got: {:?}",
        names,
    );
    assert!(
        !names.contains(&"innerOnly"),
        "Outer closure body should not see Inner's members, got: {:?}",
        names,
    );
}

/// The enclosing `@param-closure-this` call site is still found when the
/// closure containing it is not itself a call argument (here, assigned
/// to a variable rather than passed directly to `withOuter`).
#[tokio::test]
async fn test_param_closure_this_outer_closure_assigned_to_variable() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_assigned_var.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Inner {\n",
        "    public function innerOnly(): void {}\n",
        "}\n",
        "class Outer {\n",
        "    /**\n",
        "     * @param-closure-this Inner $callback\n",
        "     */\n",
        "    public function withInner(\\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function boot(Outer $outer): void {\n",
        "        $fn = function () use ($outer) {\n",
        "            $outer->withInner(function () {\n",
        "                $this->\n",
        "            });\n",
        "        };\n",
        "        $fn();\n",
        "    }\n",
        "}\n",
    );

    // Line 14: `                $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 14, 23).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"innerOnly"),
        "Expected 'innerOnly' from @param-closure-this Inner even though the outer \
         closure is assigned to a variable rather than passed as a call argument, got: {:?}",
        names,
    );
}

// ─── Narrowing on top of a rebound `$this` ──────────────────────────────────

/// `@param-closure-this` states what the closure is *bound* to, so a
/// narrowing proof inside the body still refines it.  A Pest suite writes
/// `assert($this instanceof AppTestCase)` as the closure's first line to
/// name the subclass `pest()->extends(…)` actually binds, which no
/// expression in the test file says; the members that subclass adds must
/// resolve from there rather than from the declared base.
#[tokio::test]
async fn assert_instanceof_narrows_a_rebound_this_to_the_subclass() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_narrowed.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class TestCase {\n",
        "    public function assertTrue(bool $c): void {}\n",
        "}\n",
        "class AppTestCase extends TestCase {\n",
        "    public function visitPage(string $url): void {}\n",
        "}\n",
        "/**\n",
        " * @param-closure-this TestCase $callback\n",
        " */\n",
        "function test(string $description, \\Closure $callback): void {}\n",
        "test('it works', function () {\n",
        "    assert($this instanceof AppTestCase);\n",
        "    $this->\n",
        "});\n",
    );

    // Line 13: `    $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 13, 11).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"visitPage"),
        "Expected 'visitPage' from the asserted AppTestCase subclass, got: {:?}",
        names,
    );
    assert!(
        names.contains(&"assertTrue"),
        "Expected 'assertTrue' inherited from the declared TestCase, got: {:?}",
        names,
    );
}

/// The scope's `$this` only wins when it is strictly narrower than the
/// declared binding.  Inside a closure nested in a method of an unrelated
/// class, the walker carries the lexically captured `$this`, which is
/// exactly what the tag is there to replace.
#[tokio::test]
async fn a_captured_this_does_not_override_the_declared_binding() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_this_captured.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Inner {\n",
        "    public function innerOnly(): void {}\n",
        "}\n",
        "class Outer {\n",
        "    /**\n",
        "     * @param-closure-this Inner $callback\n",
        "     */\n",
        "    public function withInner(\\Closure $callback): void {}\n",
        "}\n",
        "class App {\n",
        "    public function appOnly(): void {}\n",
        "    public function boot(Outer $outer): void {\n",
        "        $outer->withInner(function () {\n",
        "            $this->\n",
        "        });\n",
        "    }\n",
        "}\n",
    );

    // Line 14: `            $this->` — cursor after `->`
    let items = complete_at(&backend, &uri, src, 14, 19).await;
    let names = method_names(&items);
    assert!(
        names.contains(&"innerOnly"),
        "Expected 'innerOnly' from @param-closure-this Inner, got: {:?}",
        names,
    );
    assert!(
        !names.contains(&"appOnly"),
        "The lexically captured App must not survive the rebinding, got: {:?}",
        names,
    );
}

// ─── Docblock parsing unit tests ────────────────────────────────────────────

#[test]
fn test_extract_param_closure_this_basic() {
    use phpantom_lsp::docblock::extract_param_closure_this;
    use phpantom_lsp::php_type::PhpType;

    let doc = "/**\n * @param-closure-this Route $callback\n */";
    let results = extract_param_closure_this(doc);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0],
        (PhpType::parse("Route"), "$callback".to_string())
    );
}

#[test]
fn test_extract_param_closure_this_fqn() {
    use phpantom_lsp::docblock::extract_param_closure_this;
    use phpantom_lsp::php_type::PhpType;

    let doc = "/**\n * @param-closure-this \\Illuminate\\Routing\\Route $callback\n */";
    let results = extract_param_closure_this(doc);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0],
        (
            PhpType::parse("\\Illuminate\\Routing\\Route"),
            "$callback".to_string()
        )
    );
}

#[test]
fn test_extract_param_closure_this_dollar_this() {
    use phpantom_lsp::docblock::extract_param_closure_this;
    use phpantom_lsp::php_type::PhpType;

    let doc = "/**\n * @param-closure-this  $this  $callback\n */";
    let results = extract_param_closure_this(doc);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0],
        (PhpType::parse("$this"), "$callback".to_string())
    );
}

#[test]
fn test_extract_param_closure_this_static() {
    use phpantom_lsp::docblock::extract_param_closure_this;
    use phpantom_lsp::php_type::PhpType;

    let doc = "/**\n * @param-closure-this static  $macro\n */";
    let results = extract_param_closure_this(doc);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], (PhpType::parse("static"), "$macro".to_string()));
}

#[test]
fn test_extract_param_closure_this_multiple() {
    use phpantom_lsp::docblock::extract_param_closure_this;
    use phpantom_lsp::php_type::PhpType;

    let doc = concat!(
        "/**\n",
        " * @param-closure-this Route $callback\n",
        " * @param-closure-this TestCase $setup\n",
        " */",
    );
    let results = extract_param_closure_this(doc);
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0],
        (PhpType::parse("Route"), "$callback".to_string())
    );
    assert_eq!(
        results[1],
        (PhpType::parse("TestCase"), "$setup".to_string())
    );
}

#[test]
fn test_extract_param_closure_this_no_tag() {
    use phpantom_lsp::docblock::extract_param_closure_this;

    let doc = "/**\n * @param string $name\n * @return void\n */";
    let results = extract_param_closure_this(doc);
    assert!(results.is_empty());
}

#[test]
fn test_extract_param_closure_this_missing_param_name() {
    use phpantom_lsp::docblock::extract_param_closure_this;

    // No `$paramName` after the type — should be skipped.
    let doc = "/**\n * @param-closure-this Route\n */";
    let results = extract_param_closure_this(doc);
    assert!(results.is_empty());
}

#[test]
fn test_extract_param_closure_this_coexists_with_param() {
    use phpantom_lsp::docblock::extract_param_closure_this;
    use phpantom_lsp::php_type::PhpType;

    let doc = concat!(
        "/**\n",
        " * @param string $driver\n",
        " * @param \\Closure $callback\n",
        " *\n",
        " * @param-closure-this  $this  $callback\n",
        " *\n",
        " * @return $this\n",
        " */",
    );
    let results = extract_param_closure_this(doc);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0],
        (PhpType::parse("$this"), "$callback".to_string())
    );
}

#[tokio::test]
async fn param_closure_this_union_members_and_chains() {
    for call in ["bindContext", "(new Binder())->bind", "Binder::bind"] {
        for body in [
            "function () { $this->MARK; }",
            "fn() => $this->MARK",
            "function () { $this->next()->MARK; }",
        ] {
            let backend = create_test_backend();
            let uri = Url::parse("file:///test/closure_union.php").unwrap();
            let src = format!(
                r#"<?php
trait ContextTrait {{ public function fromTrait(): void {{}} }}
class BaseContext {{ public function fromBase(): void {{}} }}
class FirstContext extends BaseContext {{
    use ContextTrait;
    public string $firstProperty;
    public function firstOnly(): void {{}}
    public function next(): self {{ return $this; }}
}}
class SecondContext {{
    public string $secondProperty;
    public function secondOnly(): void {{}}
    public function next(): self {{ return $this; }}
}}
/** @param-closure-this FirstContext|SecondContext $callback */
function bindContext(\Closure $callback): void {{}}
class Binder {{
    /** @param-closure-this FirstContext|SecondContext $callback */
    public static function bind(\Closure $callback): void {{}}
}}
class Host {{
    public function lexicalOnly(): void {{}}
    public function run(): void {{ {call}({body}); }}
}}
"#
            );
            let position = marker_position(&src);
            let src = src.replace("MARK", "");
            let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
            let names = method_names(&items);
            assert!(
                !names.contains(&"lexicalOnly"),
                "lexical binding leaked: {names:?}"
            );
            for expected in ["firstOnly", "secondOnly", "fromBase", "fromTrait"] {
                assert!(
                    names.contains(&expected),
                    "{call}, {body}: missing {expected}: {names:?}"
                );
            }
            let properties = property_names(&items);
            for expected in ["firstProperty", "secondProperty"] {
                assert!(
                    properties.contains(&expected),
                    "{call}, {body}: missing {expected}: {properties:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn param_closure_this_union_narrowing_and_nested_override() {
    for (body, expected, absent) in [
        (
            "assert($this instanceof FirstContext); $this->MARK;",
            "firstOnly",
            "secondOnly",
        ),
        (
            "bindInner(function () { $this->MARK; });",
            "innerOnly",
            "firstOnly",
        ),
    ] {
        let backend = create_test_backend();
        let uri = Url::parse("file:///test/closure_union_scope.php").unwrap();
        let src = format!(
            r#"<?php
class FirstContext {{ public function firstOnly(): void {{}} }}
class SecondContext {{ public function secondOnly(): void {{}} }}
class InnerContext {{ public function innerOnly(): void {{}} }}
/** @param-closure-this FirstContext|SecondContext $callback */
function bindContext(\Closure $callback): void {{}}
/** @param-closure-this InnerContext $callback */
function bindInner(\Closure $callback): void {{}}
bindContext(function () {{ {body} }});
"#
        );
        let position = marker_position(&src);
        let src = src.replace("MARK", "");
        let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
        let names = method_names(&items);
        assert!(names.contains(&expected), "missing {expected}: {names:?}");
        assert!(!names.contains(&absent), "unexpected {absent}: {names:?}");
    }
}

#[tokio::test]
async fn param_closure_this_union_uses_declaring_imports() {
    let (backend, dir) = create_psr4_workspace(
        r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        &[
            (
                "src/Support/Binder.php",
                r#"<?php
namespace App\Support;
use App\One\Context as First;
use App\Two\Context as Second;
class Binder {
    /** @param-closure-this First|Second $callback */
    public function bind(\Closure $callback): void {}
}
"#,
            ),
            (
                "src/One/Context.php",
                "<?php\nnamespace App\\One;\nclass Context { public function firstOnly(): void {} }",
            ),
            (
                "src/Two/Context.php",
                "<?php\nnamespace App\\Two;\nclass Context { public function secondOnly(): void {} }",
            ),
        ],
    );
    let uri = Url::from_file_path(dir.path().join("src/Consumer.php")).unwrap();
    let src =
        "<?php\nnamespace App;\n(new \\App\\Support\\Binder())->bind(function () { $this->\n});\n";
    let items = complete_at(
        &backend,
        &uri,
        src,
        2,
        src.lines().nth(2).unwrap().len() as u32,
    )
    .await;
    let names = method_names(&items);
    assert!(names.contains(&"firstOnly"), "{names:?}");
    assert!(names.contains(&"secondOnly"), "{names:?}");
}

#[tokio::test]
async fn param_closure_this_union_relative_and_unresolved_members() {
    for annotation in [
        "self|OtherContext",
        "static|OtherContext",
        "$this|OtherContext",
        "Binder|OtherContext|MissingContext",
    ] {
        let backend = create_test_backend();
        let uri = Url::parse("file:///test/closure_union_relative.php").unwrap();
        let src = format!(
            r#"<?php
class OtherContext {{ public function otherOnly(): void {{}} }}
class Binder {{
    public function ownerOnly(): void {{}}
    /** @param-closure-this {annotation} $callback */
    public function bind(\Closure $callback): void {{}}
}}
(new Binder())->bind(function () {{ $this->MARK; }});
"#
        );
        let position = marker_position(&src);
        let src = src.replace("MARK", "");
        let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
        let names = method_names(&items);
        assert!(names.contains(&"ownerOnly"), "{annotation}: {names:?}");
        assert!(names.contains(&"otherOnly"), "{annotation}: {names:?}");
    }
}

fn marker_position(src: &str) -> Position {
    let (line, text) = src
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("MARK"))
        .unwrap();
    Position::new(line as u32, text.find("MARK").unwrap() as u32)
}

#[tokio::test]
async fn param_closure_this_union_does_not_narrow_to_lexical_member() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_union_lexical_member.php").unwrap();
    let src = r#"<?php
class FirstContext {
    public function firstOnly(): void {}
    public function run(): void {
        bindContext(function () { $this->MARK; });
    }
}
class SecondContext { public function secondOnly(): void {} }
/** @param-closure-this FirstContext|SecondContext $callback */
function bindContext(\Closure $callback): void {}
"#;
    let position = marker_position(src);
    let src = src.replace("MARK", "");
    let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
    let names = method_names(&items);
    assert!(names.contains(&"firstOnly"), "{names:?}");
    assert!(names.contains(&"secondOnly"), "{names:?}");
}

#[tokio::test]
async fn param_closure_this_union_guards() {
    for lexical in ["Host", "FirstContext"] {
        for (body, expected, absent) in [
            (
                "assert($this instanceof FirstContext); $this->MARK;",
                "firstOnly",
                "secondOnly",
            ),
            (
                "if ($this instanceof FirstContext) {} else { $this->MARK; }",
                "secondOnly",
                "firstOnly",
            ),
            (
                "if ($this instanceof FirstContext) { return; } $this->MARK;",
                "secondOnly",
                "firstOnly",
            ),
            (
                "if ($this instanceof FirstContext) { $nested = function () { $this->MARK; }; }",
                "firstOnly",
                "secondOnly",
            ),
        ] {
            let backend = create_test_backend();
            let uri = Url::parse("file:///test/closure_union_guards.php").unwrap();
            let run =
                format!("public function run(): void {{ bindContext(function () {{ {body} }}); }}");
            let src = format!(
                r#"<?php
class FirstContext {{ public function firstOnly(): void {{}} {} }}
class SecondContext {{ public function secondOnly(): void {{}} }}
class Host {{ {} }}
/** @param-closure-this FirstContext|SecondContext $callback */
function bindContext(\Closure $callback): void {{}}
"#,
                if lexical == "FirstContext" { &run } else { "" },
                if lexical == "Host" { &run } else { "" }
            );
            let position = marker_position(&src);
            let src = src.replace("MARK", "");
            let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
            let names = method_names(&items);
            assert!(names.contains(&expected), "{lexical}, {body}: {names:?}");
            assert!(!names.contains(&absent), "{lexical}, {body}: {names:?}");
        }
    }
}

#[tokio::test]
async fn param_closure_this_union_static_chains() {
    for keyword in ["self", "static"] {
        for expression in [
            format!("{keyword}::next()->MARK;"),
            format!("$next = {keyword}::next(); $next->MARK;"),
        ] {
            let backend = create_test_backend();
            let uri = Url::parse("file:///test/closure_union_static.php").unwrap();
            let src = format!(
                r#"<?php
class FirstContext {{
    public function firstOnly(): void {{}}
    public static function next(): self {{ return new self(); }}
}}
class SecondContext {{
    public function secondOnly(): void {{}}
    public static function next(): self {{ return new self(); }}
}}
/** @param-closure-this FirstContext|SecondContext $callback */
function bindContext(\Closure $callback): void {{}}
bindContext(function () {{ {expression} }});
"#
            );
            let position = marker_position(&src);
            let src = src.replace("MARK", "");
            let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
            let names = method_names(&items);
            assert!(names.contains(&"firstOnly"), "{expression}: {names:?}");
            assert!(names.contains(&"secondOnly"), "{expression}: {names:?}");
        }
    }
}

#[tokio::test]
async fn param_closure_this_union_nested_receivers() {
    for receiver in ["$this->", "$this?->", "self::", "static::"] {
        for annotation in ["FirstContext|SecondContext", "SecondContext|FirstContext"] {
            let backend = create_test_backend();
            let uri = Url::parse("file:///test/closure_union_nested_receivers.php").unwrap();
            let src = format!(
                r#"<?php
class FirstContext {{
    public function firstOnly(): void {{}}
    /** @param-closure-this FirstContext $callback */
    public static function bind(\Closure $callback): void {{}}
}}
class SecondContext {{
    public function secondOnly(): void {{}}
    /** @param-closure-this SecondContext $callback */
    public static function bind(\Closure $callback): void {{}}
}}
/** @param-closure-this {annotation} $callback */
function bindContext(\Closure $callback): void {{}}
bindContext(function () {{ {receiver}bind(function () {{ $this->MARK; }}); }});
"#
            );
            let position = marker_position(&src);
            let src = src.replace("MARK", "");
            let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
            let names = method_names(&items);
            assert!(names.contains(&"firstOnly"), "{annotation}: {names:?}");
            assert!(names.contains(&"secondOnly"), "{annotation}: {names:?}");
        }
    }
}

#[tokio::test]
async fn param_closure_this_union_arrow_guard() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/closure_union_arrow.php").unwrap();
    let src = r#"<?php
class FirstContext { public function firstOnly(): void {} }
class SecondContext { public function secondOnly(): void {} }
/** @param-closure-this FirstContext|SecondContext $callback */
function bindContext(\Closure $callback): void {}
bindContext(fn() => $this instanceof FirstContext ? null : $this->MARK);
"#;
    let position = marker_position(src);
    let src = src.replace("MARK", "");
    let items = complete_at(&backend, &uri, &src, position.line, position.character).await;
    let names = method_names(&items);
    assert!(names.contains(&"secondOnly"), "{names:?}");
    assert!(!names.contains(&"firstOnly"), "{names:?}");
}
