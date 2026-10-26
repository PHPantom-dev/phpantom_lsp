use std::collections::HashMap;
use std::sync::Arc;

use super::*;
use crate::atom::atom;
use crate::class_lookup::{Fqn, ResolvedClassLoader};
use crate::php_type::PhpType;
use crate::types::{ClassInfo, FunctionInfo, MethodExtras, MethodInfo};
use tower_lsp_server::gen_lsp_types::Position;

// ── Low-level scanning tests ────────────────────────────────────────

#[test]
fn test_find_throw_statements_basic() {
    let body = r#"
        throw new InvalidArgumentException("bad");
        throw new \RuntimeException("oops");
    "#;
    let throws = find_throw_statements(body);
    assert_eq!(throws.len(), 2);
    assert_eq!(throws[0], PhpType::named(atom("InvalidArgumentException")));
    assert_eq!(throws[1], PhpType::named(atom("\\RuntimeException")));
}

#[test]
fn test_find_throw_statements_skips_strings() {
    let body = r#"
        $msg = "throw new FakeException()";
        throw new RealException("msg");
    "#;
    let throws = find_throw_statements(body);
    assert_eq!(throws.len(), 1);
    assert_eq!(throws[0], PhpType::named(atom("RealException")));
}

#[test]
fn test_find_throw_statements_skips_comments() {
    let body = r#"
        // throw new CommentException();
        /* throw new BlockException(); */
        # throw new HashException();
        throw new RealException("msg");
    "#;
    let throws = find_throw_statements(body);
    assert_eq!(throws.len(), 1);
    assert_eq!(throws[0], PhpType::named(atom("RealException")));
}

#[test]
fn test_find_method_throws_tags_basic() {
    let content = r#"<?php
/**
 * @throws InvalidArgumentException
 * @throws \RuntimeException
 */
public function doSomething(): void {
}
    "#;
    let tags = find_method_throws_tags(content, "doSomething");
    assert_eq!(
        tags,
        vec![
            PhpType::named(atom("InvalidArgumentException")),
            PhpType::named(atom("RuntimeException"))
        ]
    );
}

#[test]
fn test_find_method_throws_tags_with_modifiers() {
    let content = r#"<?php
/**
 * @throws InvalidArgumentException
 */
private static function doSomething(): void {
}
    "#;
    let tags = find_method_throws_tags(content, "doSomething");
    assert_eq!(tags, vec![PhpType::named(atom("InvalidArgumentException"))]);
}

#[test]
fn test_find_method_return_type_native() {
    let content = r#"
public function createException(): RuntimeException {
}
    "#;
    let ret = find_method_return_type(content, "createException");
    assert_eq!(ret, Some(PhpType::named(atom("RuntimeException"))));
}

#[test]
fn test_find_method_return_type_docblock() {
    let content = r#"<?php
/**
 * @return RuntimeException
 */
public function createException() {
}
    "#;
    let ret = find_method_return_type(content, "createException");
    assert_eq!(ret, Some(PhpType::named(atom("RuntimeException"))));
}

#[test]
fn test_find_method_return_type_skips_void() {
    let content = r#"<?php
/**
 * @return void
 */
public function doNothing() {
}
    "#;
    let ret = find_method_return_type(content, "doNothing");
    assert_eq!(ret, None);
}

#[test]
fn test_find_inline_throws_annotations() {
    let body = r#"
        /** @throws InvalidArgumentException */
        $client->request();
        /** @throws RuntimeException when things go wrong */
        $db->query();
    "#;
    let annotations = find_inline_throws_annotations(body);
    let names: Vec<String> = annotations.iter().map(|t| t.to_string()).collect();
    assert_eq!(names, vec!["InvalidArgumentException", "RuntimeException"]);
}

#[test]
fn test_find_propagated_throws() {
    let file_content = r#"<?php
/**
 * @throws IOException
 * @throws NetworkException
 */
public function riskyMethod(): void {
    // ...
}

public function caller(): void {
    $this->riskyMethod();
}
    "#;
    // Scan the body of `caller`
    let body = "$this->riskyMethod();";
    let propagated = find_propagated_throws(body, file_content);
    let names: Vec<String> = propagated.iter().map(|t| t.to_string()).collect();
    assert_eq!(names, vec!["IOException", "NetworkException"]);
}

#[test]
fn test_find_throw_expression_types() {
    let file_content = r#"
public function createException(): RuntimeException {
    return new RuntimeException("oops");
}

public function caller(): void {
    throw $this->createException();
}
    "#;
    let body = "throw $this->createException();";
    let types = find_throw_expression_types(body, file_content);
    assert_eq!(types.len(), 1);
    assert_eq!(types[0], PhpType::named(atom("RuntimeException")));
}

/// A plain comment above an undocumented method is not its docblock, and
/// neither is the docblock of the method before it.
#[test]
fn test_find_method_throws_tags_skips_a_plain_comment() {
    let content = r#"<?php
class Foo {
    /** @throws IOException */
    public function read(): void {}

    /* Writes the buffer. */
    public function write(): void {}
}
"#;
    assert!(find_method_throws_tags(content, "write").is_empty());
    assert_eq!(
        find_method_throws_tags(content, "read"),
        vec![PhpType::named(atom("IOException"))]
    );
}

/// A comment or a string that names the method is not its declaration.
#[test]
fn test_find_method_throws_tags_skips_comments_and_strings_naming_it() {
    let content = r#"<?php
class Foo {
    // Call function write() before closing.
    private string $doc = 'function write()';

    /** @throws IOException */
    public function &write(): array {}
}
"#;
    assert_eq!(
        find_method_throws_tags(content, "write"),
        vec![PhpType::named(atom("IOException"))]
    );
}

#[test]
fn test_find_method_return_type_with_nested_parens() {
    let content = r#"
public function createException(array $opts = array()): RuntimeException {
}
    "#;
    let ret = find_method_return_type(content, "createException");
    assert_eq!(ret, Some(PhpType::named(atom("RuntimeException"))));
}

// ── Uncaught throws ─────────────────────────────────────────────────

/// The cursor inside the first docblock of `content`, on its `@` line.
fn docblock_cursor(content: &str) -> Position {
    let line = content
        .lines()
        .position(|line| line.trim() == "* @")
        .expect("a `* @` line in the fixture");
    Position {
        line: line as u32,
        character: 5,
    }
}

/// The uncaught exception names `content`'s documented function throws,
/// with nothing to resolve names or calls with.
fn uncaught_names(content: &str) -> Vec<String> {
    find_uncaught_throw_types(content, docblock_cursor(content))
        .iter()
        .map(|ty| ty.to_string())
        .collect()
}

/// The uncaught exception names, resolving names and calls with the given
/// loaders.
fn uncaught_names_with(
    content: &str,
    class_loader: &ResolvedClassLoader<'_>,
    function_loader: crate::types::FunctionLoader<'_>,
    use_map: &HashMap<String, String>,
) -> Vec<String> {
    find_uncaught_throw_types_with_context(
        content,
        docblock_cursor(content),
        Some(&ThrowsContext {
            class_loader: crate::test_fixtures::fqn_source_loader(class_loader),
            resolved_class_loader: class_loader,
            function_loader,
            use_map,
            local_classes: &[],
            backend: None,
        }),
    )
    .iter()
    .map(|ty| ty.to_string())
    .collect()
}

/// The uncaught exception names, with a backend that has indexed
/// `content` resolving the body's names and calls, as in the editor.
fn uncaught_names_indexed(content: &str) -> Vec<String> {
    uncaught_names_in(&crate::Backend::new_test(), content)
}

/// [`uncaught_names_indexed`] with `backend`, such as one that knows the
/// standard library's classes.
fn uncaught_names_in(backend: &crate::Backend, content: &str) -> Vec<String> {
    let uri = "file:///throws.php";
    backend.update_ast(uri, content);
    let file_ctx = backend.file_context(uri);
    let class_loader = backend.class_loader(&file_ctx);
    let resolved_class_loader = backend.resolved_class_loader(&file_ctx);
    let function_loader = backend.function_loader(&file_ctx);
    find_uncaught_throw_types_with_context(
        content,
        docblock_cursor(content),
        Some(&ThrowsContext {
            class_loader: &class_loader,
            resolved_class_loader: &resolved_class_loader,
            function_loader: Some(&function_loader),
            use_map: &file_ctx.use_map,
            local_classes: &file_ctx.classes,
            backend: Some(backend),
        }),
    )
    .iter()
    .map(|ty| ty.to_string())
    .collect()
}

#[test]
fn caught_throw_is_not_reported() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw new InvalidArgumentException(\"bad\");\n",
        "        } catch (InvalidArgumentException $e) {\n",
        "            // handled\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    assert!(uncaught_names(content).is_empty());
}

#[test]
fn uncaught_throw_is_reported() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        throw new RuntimeException(\"oops\");\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(uncaught_names(content), vec!["RuntimeException"]);
}

#[test]
fn only_the_throw_outside_the_try_is_reported() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw new InvalidArgumentException(\"bad\");\n",
        "        } catch (InvalidArgumentException $e) {\n",
        "            // handled\n",
        "        }\n",
        "        throw new RuntimeException(\"oops\");\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(uncaught_names(content), vec!["RuntimeException"]);
}

#[test]
fn inline_throws_annotation_inside_a_try_is_caught() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            /** @throws NotFoundException */\n",
        "            findOrFail();\n",
        "        } catch (NotFoundException $e) {\n",
        "            // handled\n",
        "        }\n",
        "        /** @throws RuntimeException */\n",
        "        riskyCall();\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(uncaught_names(content), vec!["RuntimeException"]);
}

#[test]
fn rethrown_catch_variable_throws_the_caught_type() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            doSomething();\n",
        "        } catch (ValidationException $e) {\n",
        "            throw $e;\n",
        "        } catch (NotFoundException $e) {\n",
        "            throw $e;\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(
        uncaught_names(content),
        vec!["ValidationException", "NotFoundException"]
    );
}

#[test]
fn swallowed_catch_variable_is_not_reported() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        try {\n",
        "            throw new ValidationException('bad');\n",
        "        } catch (ValidationException $e) {\n",
        "            // swallowed\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    assert!(uncaught_names(content).is_empty());
}

#[test]
fn throw_in_a_string_comment_or_closure_is_not_reported() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function bar(): void {\n",
        "    echo 'throw new StringException();';\n",
        "    // throw new CommentException();\n",
        "    $later = function () { throw new ClosureException(); };\n",
        "}\n",
    );
    assert!(uncaught_names(content).is_empty());
}

#[test]
fn an_immediately_invoked_closure_throws_where_it_is_written() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function bar(): void {\n",
        "    (function () { throw new InvokedException(); })();\n",
        "}\n",
    );
    assert_eq!(uncaught_names(content), vec!["InvokedException"]);
}

#[test]
fn the_function_after_a_docblock_being_opened_is_found() {
    // Completion on `/**` fires before the editor closes the comment.
    let content = concat!(
        "<?php\n",
        "/**\n",
        "function first(): void {\n",
        "    throw new FirstException();\n",
        "}\n",
        "/** Second. */\n",
        "function second(): void {\n",
        "    throw new SecondException();\n",
        "}\n",
    );
    let uncaught = find_uncaught_throw_types(content, Position::new(1, 3));
    assert_eq!(uncaught, vec![PhpType::named(atom("FirstException"))]);
}

#[test]
fn the_lines_of_a_docblock_being_typed_are_not_code() {
    let content = concat!(
        "<?php\n",
        "class Foo {\n",
        "    /**\n",
        "     * @thr\n",
        "     * @param int $x\n",
        "    public function bar(int $x): void {\n",
        "        throw new BarException();\n",
        "    }\n",
        "}\n",
    );
    let uncaught = find_uncaught_throw_types(content, Position::new(3, 11));
    assert_eq!(uncaught, vec![PhpType::named(atom("BarException"))]);
}

/// A `/*` in a string or in the docblock's own text opens no comment.
#[test]
fn a_comment_opener_in_a_string_or_the_docblock_text_is_not_one() {
    let content = concat!(
        "<?php\n",
        "$accept = 'image/*';\n",
        "/**\n",
        " * @\n",
        " * @param string $pattern like src/*.php\n",
        " */\n",
        "function scan(string $pattern): void {\n",
        "    throw new ScanException();\n",
        "}\n",
    );
    assert_eq!(uncaught_names(content), vec!["ScanException"]);
    // "Update docblock" asks from the start of the docblock.
    let uncaught = find_uncaught_throw_types(content, Position::new(2, 0));
    assert_eq!(uncaught, vec![PhpType::named(atom("ScanException"))]);
}

#[test]
fn a_throw_whose_return_type_names_the_exception_is_reported() {
    let content = concat!(
        "<?php\n",
        "function makeException(): RuntimeException {\n",
        "    return new RuntimeException('oops');\n",
        "}\n",
        "class Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function bar(): void {\n",
        "        throw makeException();\n",
        "    }\n",
        "    /**\n",
        "     * Caught.\n",
        "     */\n",
        "    public function baz(): void {\n",
        "        try {\n",
        "            throw makeException();\n",
        "        } catch (RuntimeException $e) {\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(uncaught_names_indexed(content), vec!["RuntimeException"]);
    let caught = content
        .replace("     * @\n", "     * Bar.\n")
        .replace("     * Caught.\n", "     * @\n");
    assert!(uncaught_names_indexed(&caught).is_empty());
}

#[test]
fn same_class_methods_propagate_their_throws() {
    let content = concat!(
        "<?php\n",
        "class Repo {\n",
        "    /** @throws NotFoundException */\n",
        "    public function find(int $id): object {}\n",
        "}\n",
        "class Service {\n",
        "    private Repo $repo;\n",
        "    /** @throws LockedException */\n",
        "    private function lock(): void {}\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function load(int $id): object {\n",
        "        $this->lock();\n",
        "        return $this->repo->find($id);\n",
        "    }\n",
        "}\n",
    );
    assert_eq!(
        uncaught_names_indexed(content),
        vec!["LockedException", "NotFoundException"]
    );
}

#[test]
fn a_variable_assigned_from_a_factory_is_typed_for_its_calls() {
    let content = concat!(
        "<?php\n",
        "class Client {\n",
        "    /** @throws TimeoutException */\n",
        "    public function send(): void {}\n",
        "}\n",
        "function client(): Client { return new Client(); }\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function run(): void {\n",
        "    $client = client();\n",
        "    $client->send();\n",
        "}\n",
    );
    assert_eq!(uncaught_names_indexed(content), vec!["TimeoutException"]);
}

#[test]
fn a_catch_of_a_parent_class_catches_its_subclasses() {
    let content = concat!(
        "<?php\n",
        "class BaseException extends Exception {}\n",
        "class ChildException extends BaseException {}\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function run(): void {\n",
        "    try {\n",
        "        throw new ChildException();\n",
        "    } catch (BaseException $e) {\n",
        "    }\n",
        "}\n",
    );
    assert!(uncaught_names_indexed(content).is_empty());
}

#[test]
fn a_catch_of_exception_does_not_take_an_error() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function run(): void {\n",
        "    try {\n",
        "        throw new TypeError();\n",
        "    } catch (Exception $e) {\n",
        "    }\n",
        "}\n",
    );
    let backend = crate::Backend::new_test_with_full_stubs();
    assert_eq!(uncaught_names_in(&backend, content), vec!["TypeError"]);
}

// ── Callee `@throws` propagation ────────────────────────────────────

fn make_class_with_throws(name: &str, methods: Vec<(&str, Vec<&str>)>) -> Arc<ClassInfo> {
    let method_infos: Vec<MethodInfo> = methods
        .into_iter()
        .map(|(method_name, throws)| MethodInfo {
            name: crate::atom::atom(method_name),
            extras: MethodExtras {
                throws: throws.into_iter().map(PhpType::parse).collect(),
                ..Default::default()
            }
            .into_option(),
            ..Default::default()
        })
        .collect();

    Arc::new(ClassInfo {
        name: crate::atom::atom(name),
        methods: method_infos
            .into_iter()
            .map(Arc::new)
            .collect::<Vec<_>>()
            .into(),
        ..Default::default()
    })
}

fn loader_for(classes: Vec<Arc<ClassInfo>>) -> impl Fn(Fqn<'_>) -> Option<Arc<ClassInfo>> {
    move |name: Fqn<'_>| {
        let name = name.as_str();
        classes
            .iter()
            .find(|cls| cls.fqn() == name.trim_start_matches('\\'))
            .cloned()
    }
}

#[test]
fn a_parameter_receiver_propagates_the_method_throws() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function handle(BusinessCentralService $service): void {\n",
        "    $service->sendData($customer, Kind::CREATE);\n",
        "}\n",
    );
    let class_loader = loader_for(vec![make_class_with_throws(
        "BusinessCentralService",
        vec![(
            "sendData",
            vec!["BusinessCentralException", "ConvertException"],
        )],
    )]);
    assert_eq!(
        uncaught_names_with(content, &class_loader, None, &HashMap::new()),
        vec!["BusinessCentralException", "ConvertException"]
    );
}

#[test]
fn unknown_receivers_properties_and_quiet_methods_add_nothing() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function handle(MyService $service): void {\n",
        "    $unknown->doStuff();\n",
        "    $service->someProperty;\n",
        "    $service->safeMethod();\n",
        "    if ($x) {}\n",
        "    foreach ($items as $item) {}\n",
        "}\n",
    );
    let class_loader = loader_for(vec![make_class_with_throws(
        "MyService",
        vec![("safeMethod", vec![])],
    )]);
    assert!(uncaught_names_with(content, &class_loader, None, &HashMap::new()).is_empty());
}

#[test]
fn each_exception_is_reported_once_in_the_order_first_thrown() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function handle(MyService $service): void {\n",
        "    $service->methodA();\n",
        "    $service->methodB();\n",
        "    $service->methodA();\n",
        "}\n",
    );
    let class_loader = loader_for(vec![make_class_with_throws(
        "MyService",
        vec![
            ("methodA", vec!["IOException"]),
            ("methodB", vec!["NetworkException"]),
        ],
    )]);
    assert_eq!(
        uncaught_names_with(content, &class_loader, None, &HashMap::new()),
        vec!["IOException", "NetworkException"]
    );
}

#[test]
fn static_calls_constructors_and_functions_propagate_their_throws() {
    let content = concat!(
        "<?php\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function handle(): void {\n",
        "    BusinessCentralService::validate();\n",
        "    new HttpClient();\n",
        "    helperFunction();\n",
        "}\n",
    );
    let class_loader = loader_for(vec![
        make_class_with_throws(
            "BusinessCentralService",
            vec![("validate", vec!["ValidationException"])],
        ),
        make_class_with_throws(
            "HttpClient",
            vec![("__construct", vec!["ConnectionException"])],
        ),
    ]);
    let helper = FunctionInfo {
        name: atom("helperFunction"),
        throws: vec![PhpType::parse("HelperException")],
        ..Default::default()
    };
    let function_loader = move |name: &str, _offset: u32| -> Option<FunctionInfo> {
        (name.trim_start_matches('\\') == "helperFunction").then(|| helper.clone())
    };
    assert_eq!(
        uncaught_names_with(
            content,
            &class_loader,
            Some(&function_loader),
            &HashMap::new()
        ),
        vec![
            "ValidationException",
            "ConnectionException",
            "HelperException"
        ]
    );
}

/// A parameter's class hint is resolved through the file's imports and
/// namespace, and a leading `\` keeps it exact.
#[test]
fn parameter_hints_resolve_through_imports_and_namespace() {
    let content = concat!(
        "<?php\n",
        "namespace App;\n",
        "use Lib\\Svc;\n",
        "/**\n",
        " * @\n",
        " */\n",
        "function handle(Svc $a, Local $b, \\Other $c): void {\n",
        "    $a->run();\n",
        "    $b->run();\n",
        "    $c->run();\n",
        "}\n",
    );
    let mut svc = (*make_class_with_throws("Svc", vec![("run", vec!["SvcException"])])).clone();
    svc.file_namespace = Some(atom("Lib"));
    let mut local =
        (*make_class_with_throws("Local", vec![("run", vec!["LocalException"])])).clone();
    local.file_namespace = Some(atom("App"));
    let other = make_class_with_throws("Other", vec![("run", vec!["OtherException"])]);
    let class_loader = loader_for(vec![Arc::new(svc), Arc::new(local), other]);
    let use_map = HashMap::from([("Svc".to_string(), "Lib\\Svc".to_string())]);
    assert_eq!(
        uncaught_names_with(content, &class_loader, None, &use_map),
        vec!["SvcException", "LocalException", "OtherException"]
    );
}

#[test]
fn a_propagated_throw_inside_a_try_is_caught() {
    let content = concat!(
        "<?php\nclass Foo {\n",
        "    /**\n",
        "     * @\n",
        "     */\n",
        "    public function handle(BusinessCentralService $service): void {\n",
        "        try {\n",
        "            $service->sendData();\n",
        "        } catch (RuntimeException $e) {\n",
        "            // handled\n",
        "        }\n",
        "    }\n",
        "}\n",
    );
    let class_loader = loader_for(vec![make_class_with_throws(
        "BusinessCentralService",
        vec![("sendData", vec!["RuntimeException", "ConvertException"])],
    )]);
    assert_eq!(
        uncaught_names_with(content, &class_loader, None, &HashMap::new()),
        vec!["ConvertException"]
    );
}

// ── Import helper tests ─────────────────────────────────────────────

#[test]
fn test_resolve_exception_fqn_from_use_map() {
    let mut use_map = HashMap::new();
    use_map.insert(
        "RuntimeException".to_string(),
        "App\\Exceptions\\RuntimeException".to_string(),
    );
    let result = crate::names::resolve_to_fqn("RuntimeException", &use_map, &None);
    assert_eq!(result, "App\\Exceptions\\RuntimeException".to_string());
}

#[test]
fn test_resolve_exception_fqn_from_namespace() {
    let use_map = HashMap::new();
    let ns = Some("App\\Services".to_string());
    let result = crate::names::resolve_to_fqn("CustomException", &use_map, &ns);
    assert_eq!(result, "App\\Services\\CustomException".to_string());
}

#[test]
fn test_resolve_exception_fqn_global() {
    let use_map = HashMap::new();
    let result = crate::names::resolve_to_fqn("RuntimeException", &use_map, &None);
    assert_eq!(result, "RuntimeException".to_string());
}

#[test]
fn test_has_use_import_direct() {
    let content = "<?php\nuse App\\Exceptions\\RuntimeException;\n";
    assert!(has_use_import(content, "App\\Exceptions\\RuntimeException"));
    assert!(!has_use_import(content, "App\\Exceptions\\LogicException"));
}

#[test]
fn test_has_use_import_group() {
    let content = "<?php\nuse App\\Exceptions\\{RuntimeException, LogicException};\n";
    assert!(has_use_import(content, "App\\Exceptions\\RuntimeException"));
    assert!(has_use_import(content, "App\\Exceptions\\LogicException"));
    assert!(!has_use_import(content, "App\\Exceptions\\CustomException"));
}

#[test]
fn test_has_use_import_alias() {
    let content = "<?php\nuse App\\Exceptions\\RuntimeException as RE;\n";
    assert!(has_use_import(content, "App\\Exceptions\\RuntimeException"));
}

// ── extract_throws_tags tests ───────────────────────────────────────

#[test]
fn test_extract_throws_tags_basic() {
    let docblock = "/**\n * @throws BusinessCentralException\n * @throws ConvertException\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert_eq!(
        tags,
        vec![
            PhpType::parse("BusinessCentralException"),
            PhpType::parse("ConvertException"),
        ]
    );
}

#[test]
fn test_extract_throws_tags_with_fqn() {
    let docblock = "/**\n * @throws \\App\\Exceptions\\CustomException\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert_eq!(
        tags,
        vec![PhpType::parse("\\App\\Exceptions\\CustomException")]
    );
}

#[test]
fn test_extract_throws_tags_with_description() {
    let docblock = "/**\n * @throws RuntimeException When something goes wrong\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert_eq!(tags, vec![PhpType::parse("RuntimeException")]);
}

#[test]
fn test_extract_throws_tags_empty_docblock() {
    let docblock = "/**\n * Some description\n */";
    let tags = crate::docblock::extract_throws_tags(docblock);
    assert!(tags.is_empty());
}
