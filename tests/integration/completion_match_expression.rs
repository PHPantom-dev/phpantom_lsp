use crate::common::{completion_response, create_test_backend, open_php};
use tower_lsp::lsp_types::*;

/// Match expression with `new` instantiations: each arm contributes a
/// possible type, so the variable should show completions from all arms.
#[tokio::test]
async fn test_completion_match_expression_multiple_new_instantiations() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_new.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ElasticProductReviewIndexService {\n",
        "    public function index(): void {}\n",
        "    public function reindex(): void {}\n",
        "}\n",
        "\n",
        "class ElasticBrandIndexService {\n",
        "    public function index(): void {}\n",
        "    public function bulkDelete(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(string $indexName): void {\n",
        "        $service = match ($indexName) {\n",
        "            'product-reviews' => new ElasticProductReviewIndexService(),\n",
        "            'brands'          => new ElasticBrandIndexService(),\n",
        "            default           => null,\n",
        "        };\n",
        "        $service->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$service->` on line 18
    let result = completion_response(&backend, &uri, 18, 18, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $service-> from match"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // `index` is on both classes
            assert!(
                labels.iter().any(|l| l.starts_with("index")),
                "Should include index (shared method), got: {:?}",
                labels
            );
            // `reindex` is only on ElasticProductReviewIndexService
            assert!(
                labels.iter().any(|l| l.starts_with("reindex")),
                "Should include reindex from ElasticProductReviewIndexService, got: {:?}",
                labels
            );
            // `bulkDelete` is only on ElasticBrandIndexService
            assert!(
                labels.iter().any(|l| l.starts_with("bulkDelete")),
                "Should include bulkDelete from ElasticBrandIndexService, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression with static method calls: each arm's return type
/// should contribute to the variable's union type.
#[tokio::test]
async fn test_completion_match_expression_static_method_calls() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $msg): void {}\n",
        "    /** @return static */\n",
        "    public static function create(): static { return new static(); }\n",
        "}\n",
        "\n",
        "class FileLogger extends Logger {\n",
        "    public function rotate(): void {}\n",
        "}\n",
        "\n",
        "class SyslogLogger extends Logger {\n",
        "    public function facility(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function boot(string $driver): void {\n",
        "        $logger = match ($driver) {\n",
        "            'file'   => FileLogger::create(),\n",
        "            'syslog' => SyslogLogger::create(),\n",
        "        };\n",
        "        $logger->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$logger->` on line 21
    let result = completion_response(&backend, &uri, 21, 17, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $logger-> from match with static calls"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // Both are Logger subclasses, so `log` should appear
            assert!(
                labels.iter().any(|l| l.starts_with("log")),
                "Should include log (inherited), got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("rotate")),
                "Should include rotate from FileLogger, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("facility")),
                "Should include facility from SyslogLogger, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression with a single arm: the variable should resolve to
/// just that one class.
#[tokio::test]
async fn test_completion_match_expression_single_arm() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_single.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Processor {\n",
        "    public function process(): void {}\n",
        "    public function flush(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(int $mode): void {\n",
        "        $proc = match ($mode) {\n",
        "            1 => new Processor(),\n",
        "            default => new Processor(),\n",
        "        };\n",
        "        $proc->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$proc->` on line 12
    let result = completion_response(&backend, &uri, 12, 16, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $proc->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("process")),
                "Should include process from Processor, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("flush")),
                "Should include flush from Processor, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression with `$this->method()` call as arm body: the method's
/// return type should resolve correctly.
#[tokio::test]
async fn test_completion_match_expression_this_method_call() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_method.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class CacheDriver {\n",
        "    public function get(string $key): void {}\n",
        "    public function set(string $key, mixed $val): void {}\n",
        "}\n",
        "\n",
        "class QueueDriver {\n",
        "    public function push(string $job): void {}\n",
        "    public function pop(): void {}\n",
        "}\n",
        "\n",
        "class Factory {\n",
        "    /** @return CacheDriver */\n",
        "    public function makeCache(): CacheDriver { return new CacheDriver(); }\n",
        "    /** @return QueueDriver */\n",
        "    public function makeQueue(): QueueDriver { return new QueueDriver(); }\n",
        "\n",
        "    public function resolve(string $type): void {\n",
        "        $driver = match ($type) {\n",
        "            'cache' => $this->makeCache(),\n",
        "            'queue' => $this->makeQueue(),\n",
        "        };\n",
        "        $driver->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$driver->` on line 22
    let result = completion_response(&backend, &uri, 22, 17, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $driver-> from match with method calls"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // CacheDriver methods
            assert!(
                labels.iter().any(|l| l.starts_with("get")),
                "Should include get from CacheDriver, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("set")),
                "Should include set from CacheDriver, got: {:?}",
                labels
            );
            // QueueDriver methods
            assert!(
                labels.iter().any(|l| l.starts_with("push")),
                "Should include push from QueueDriver, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("pop")),
                "Should include pop from QueueDriver, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression at the top level (outside a class): `$var = match(…) { … }`
/// should still resolve all arm types.
#[tokio::test]
async fn test_completion_match_expression_top_level() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_top_level.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Alpha {\n",
        "    public function alphaMethod(): void {}\n",
        "}\n",
        "\n",
        "class Beta {\n",
        "    public function betaMethod(): void {}\n",
        "}\n",
        "\n",
        "$x = match (true) {\n",
        "    true  => new Alpha(),\n",
        "    false => new Beta(),\n",
        "};\n",
        "$x->\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$x->` on line 13
    let result = completion_response(&backend, &uri, 13, 4, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for top-level $x-> from match"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("alphaMethod")),
                "Should include alphaMethod from Alpha, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("betaMethod")),
                "Should include betaMethod from Beta, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression with property access in an arm body:
/// `'key' => $this->prop` where `prop` is typed should resolve correctly.
#[tokio::test]
async fn test_completion_match_expression_property_access() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_prop.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Connection {\n",
        "    public function query(string $sql): void {}\n",
        "}\n",
        "\n",
        "class Mailer {\n",
        "    public function send(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    /** @var Connection */\n",
        "    private Connection $db;\n",
        "    /** @var Mailer */\n",
        "    private Mailer $mailer;\n",
        "\n",
        "    public function resolve(string $name): void {\n",
        "        $svc = match ($name) {\n",
        "            'db'     => $this->db,\n",
        "            'mailer' => $this->mailer,\n",
        "        };\n",
        "        $svc->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$svc->` on line 20
    let result = completion_response(&backend, &uri, 20, 14, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $svc-> from match with property access"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("query")),
                "Should include query from Connection, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("send")),
                "Should include send from Mailer, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Cross-file match expression: arm bodies that instantiate classes from
/// other files should still resolve via PSR-4.
#[tokio::test]
async fn test_completion_match_expression_cross_file() {
    use crate::common::create_psr4_workspace;

    let composer = r#"{
        "autoload": {
            "psr-4": {
                "App\\": "src/"
            }
        }
    }"#;

    let handler_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use App\\Services\\SearchService;\n",
        "use App\\Services\\IndexService;\n",
        "\n",
        "class Handler {\n",
        "    public function handle(string $action): void {\n",
        "        $svc = match ($action) {\n",
        "            'search' => new SearchService(),\n",
        "            'index'  => new IndexService(),\n",
        "        };\n",
        "        $svc->\n",
        "    }\n",
        "}\n",
    );

    let search_service_php = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "\n",
        "class SearchService {\n",
        "    public function search(string $query): void {}\n",
        "    public function suggest(string $prefix): void {}\n",
        "}\n",
    );

    let index_service_php = concat!(
        "<?php\n",
        "namespace App\\Services;\n",
        "\n",
        "class IndexService {\n",
        "    public function reindex(): void {}\n",
        "    public function drop(): void {}\n",
        "}\n",
    );

    let (backend, _dir) = create_psr4_workspace(
        composer,
        &[
            ("src/Handler.php", handler_php),
            ("src/Services/SearchService.php", search_service_php),
            ("src/Services/IndexService.php", index_service_php),
        ],
    );

    let uri = Url::parse("file:///src/Handler.php").unwrap();
    open_php(&backend, &uri, handler_php).await;

    // Cursor after `$svc->` on line 12
    let result = completion_response(&backend, &uri, 12, 14, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for cross-file match expression"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("search")),
                "Should include search from SearchService, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("suggest")),
                "Should include suggest from SearchService, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("reindex")),
                "Should include reindex from IndexService, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("drop")),
                "Should include drop from IndexService, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression followed by an unconditional reassignment: the match
/// types should be replaced by the final assignment.
#[tokio::test]
async fn test_completion_match_expression_overridden_by_reassignment() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_override.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public function fooMethod(): void {}\n",
        "}\n",
        "\n",
        "class Bar {\n",
        "    public function barMethod(): void {}\n",
        "}\n",
        "\n",
        "class Baz {\n",
        "    public function bazMethod(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(int $x): void {\n",
        "        $obj = match ($x) {\n",
        "            1 => new Foo(),\n",
        "            2 => new Bar(),\n",
        "        };\n",
        "        $obj = new Baz();\n",
        "        $obj->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$obj->` on line 20
    let result = completion_response(&backend, &uri, 20, 14, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $obj->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // The unconditional reassignment `$obj = new Baz()` should
            // override the match expression types.
            assert!(
                labels.iter().any(|l| l.starts_with("bazMethod")),
                "Should include bazMethod from Baz (final assignment), got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("fooMethod")),
                "Should NOT include fooMethod from Foo (overridden), got: {:?}",
                labels
            );
            assert!(
                !labels.iter().any(|l| l.starts_with("barMethod")),
                "Should NOT include barMethod from Bar (overridden), got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression with three distinct arms to verify all types accumulate.
#[tokio::test]
async fn test_completion_match_expression_three_arms() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_three.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Redis {\n",
        "    public function redisGet(): void {}\n",
        "}\n",
        "\n",
        "class Memcached {\n",
        "    public function memGet(): void {}\n",
        "}\n",
        "\n",
        "class FileCache {\n",
        "    public function fileGet(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function make(string $driver): void {\n",
        "        $cache = match ($driver) {\n",
        "            'redis'     => new Redis(),\n",
        "            'memcached' => new Memcached(),\n",
        "            'file'      => new FileCache(),\n",
        "        };\n",
        "        $cache->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$cache->` on line 20
    let result = completion_response(&backend, &uri, 20, 16, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $cache-> with three match arms"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("redisGet")),
                "Should include redisGet from Redis, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("memGet")),
                "Should include memGet from Memcached, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("fileGet")),
                "Should include fileGet from FileCache, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression that uses `app()->make(Foo::class)` style calls in
/// arms — tests the text-based method call resolution fallback path.
#[tokio::test]
async fn test_completion_match_expression_method_call_on_variable() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_method_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Printer {\n",
        "    public function print(): void {}\n",
        "}\n",
        "\n",
        "class Scanner {\n",
        "    public function scan(): void {}\n",
        "}\n",
        "\n",
        "class DeviceFactory {\n",
        "    /** @return Printer */\n",
        "    public function createPrinter(): Printer { return new Printer(); }\n",
        "    /** @return Scanner */\n",
        "    public function createScanner(): Scanner { return new Scanner(); }\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(string $type): void {\n",
        "        $factory = new DeviceFactory();\n",
        "        $device = match ($type) {\n",
        "            'print' => $factory->createPrinter(),\n",
        "            'scan'  => $factory->createScanner(),\n",
        "        };\n",
        "        $device->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$device->` on line 23
    let result = completion_response(&backend, &uri, 23, 17, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $device-> from match with method calls on variable"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.iter().any(|l| l.starts_with("print")),
                "Should include print from Printer, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("scan")),
                "Should include scan from Scanner, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// Match expression inside a conditional block: the match variable's
/// types should be treated as conditional (appended to, not replacing).
#[tokio::test]
async fn test_completion_match_expression_inside_if_block() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_in_if.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class DefaultService {\n",
        "    public function defaultOp(): void {}\n",
        "}\n",
        "\n",
        "class SpecialA {\n",
        "    public function specialA(): void {}\n",
        "}\n",
        "\n",
        "class SpecialB {\n",
        "    public function specialB(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(bool $cond, int $x): void {\n",
        "        $svc = new DefaultService();\n",
        "        if ($cond) {\n",
        "            $svc = match ($x) {\n",
        "                1 => new SpecialA(),\n",
        "                2 => new SpecialB(),\n",
        "            };\n",
        "        }\n",
        "        $svc->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    // Cursor after `$svc->` on line 22
    let result = completion_response(&backend, &uri, 22, 14, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $svc-> with match inside if"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
            // DefaultService from before the if, plus SpecialA and SpecialB
            // from the conditional match arms
            assert!(
                labels.iter().any(|l| l.starts_with("defaultOp")),
                "Should include defaultOp from DefaultService, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("specialA")),
                "Should include specialA from SpecialA, got: {:?}",
                labels
            );
            assert!(
                labels.iter().any(|l| l.starts_with("specialB")),
                "Should include specialB from SpecialB, got: {:?}",
                labels
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}

/// When a variable holds a union type from a match expression, branch-only
/// members should show the originating class in `label_details.description`.
/// Intersection members should not have a class-name description.
#[tokio::test]
async fn test_completion_match_expression_detail_shows_merged_classes() {
    let backend = create_test_backend();

    let uri = Url::parse("file:///match_detail_merge.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class ElasticProductReviewIndexService {\n",
        "    public function index(): void {}\n",
        "    public function reindex(): void {}\n",
        "}\n",
        "\n",
        "class ElasticBrandIndexService {\n",
        "    public function index(): void {}\n",
        "    public function bulkDelete(): void {}\n",
        "}\n",
        "\n",
        "class App {\n",
        "    public function run(string $indexName): void {\n",
        "        $service = match ($indexName) {\n",
        "            'product-reviews' => new ElasticProductReviewIndexService(),\n",
        "            'brands'          => new ElasticBrandIndexService(),\n",
        "            default           => null,\n",
        "        };\n",
        "        $service->\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 18, 18, None).await;
    assert!(
        result.is_some(),
        "Completion should return results for $service->"
    );

    match result.unwrap() {
        CompletionResponse::Array(items) => {
            // `index` is on both classes — intersection member should
            // show both class names in label_details.
            let index_item = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("index"))
                .expect("Should have an index completion item");
            let index_desc = index_item
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref())
                .unwrap_or("");
            assert!(
                index_desc.contains("ElasticProductReviewIndexService")
                    && index_desc.contains("ElasticBrandIndexService"),
                "Shared method 'index' should show both class names, got: {:?}",
                index_desc
            );

            // `reindex` is only on ElasticProductReviewIndexService —
            // branch-only member should show that class in label_details.
            let reindex_item = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("reindex"))
                .expect("Should have a reindex completion item");
            let reindex_desc = reindex_item
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref())
                .unwrap_or("");
            assert!(
                reindex_desc.contains("ElasticProductReviewIndexService")
                    && !reindex_desc.contains("ElasticBrandIndexService"),
                "Branch-only method 'reindex' should show only its class in label_details, got: {:?}",
                reindex_desc
            );

            // `bulkDelete` is only on ElasticBrandIndexService.
            let bulk_item = items
                .iter()
                .find(|i| i.filter_text.as_deref() == Some("bulkDelete"))
                .expect("Should have a bulkDelete completion item");
            let bulk_desc = bulk_item
                .label_details
                .as_ref()
                .and_then(|ld| ld.description.as_deref())
                .unwrap_or("");
            assert!(
                bulk_desc.contains("ElasticBrandIndexService")
                    && !bulk_desc.contains("ElasticProductReviewIndexService"),
                "Branch-only method 'bulkDelete' should show only its class in label_details, got: {:?}",
                bulk_desc
            );
        }
        _ => panic!("Expected CompletionResponse::Array"),
    }
}
