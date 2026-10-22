use crate::common::{create_test_backend, create_test_backend_with_function_stubs, open_php};
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

/// Helper: open a file and request signature help at the given line/character.
async fn sig_help_at(
    backend: &phpantom_lsp::Backend,
    uri: &Url,
    text: &str,
    line: u32,
    character: u32,
) -> Option<SignatureHelp> {
    open_php(backend, uri, text).await;

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

/// Extract the active parameter index from a SignatureHelp response.
fn active_param(sh: &SignatureHelp) -> u32 {
    sh.active_parameter.unwrap_or(0)
}

/// Extract the signature label from the first (and usually only) signature.
fn sig_label(sh: &SignatureHelp) -> &str {
    &sh.signatures[0].label
}

/// Extract parameter labels as strings from the first signature.
fn param_labels(sh: &SignatureHelp) -> Vec<String> {
    let sig = &sh.signatures[0];
    let params = sig.parameters.as_ref().unwrap();
    params
        .iter()
        .map(|pi| match &pi.label {
            ParameterLabel::Simple(s) => s.clone(),
            ParameterLabel::LabelOffsets([start, end]) => {
                sig.label[*start as usize..*end as usize].to_string()
            }
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
//  Same-file function
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn standalone_function_first_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function greet(string $name, int $age): void {}\n",
        "greet(\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 6).await.unwrap();
    assert_eq!(sig_label(&sh), "(string $name, int $age): void");
    assert_eq!(active_param(&sh), 0);
    let pl = param_labels(&sh);
    assert_eq!(pl, vec!["string $name", "int $age"]);
}

#[tokio::test]
async fn imported_function_shadows_a_global_one() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_imported_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace {\n",
        "    function greet(int $times): void {}\n",
        "}\n",
        "namespace Lib {\n",
        "    function greet(string $name): void {}\n",
        "}\n",
        "namespace App {\n",
        "    use function Lib\\greet;\n",
        "    greet();\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 9, 10).await.unwrap();
    assert_eq!(sig_label(&sh), "(string $name): void");
}

/// Until its `)` is typed, a call swallows the `}` of the braced block it is
/// in.  It still resolves against that block's imports, not the namespace
/// of the block before it.
#[tokio::test]
async fn unclosed_call_resolves_through_its_braced_blocks_imports() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_unclosed_braced_import.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Lib {\n",
        "    function greet(string $name): void {}\n",
        "}\n",
        "namespace Other {\n",
        "    function greet(int $count): void {}\n",
        "}\n",
        "namespace App {\n",
        "    use function Lib\\greet;\n",
        "    greet(\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 9, 10).await.unwrap();
    assert_eq!(sig_label(&sh), "(string $name): void");
}

#[tokio::test]
async fn unclosed_call_to_a_function_its_braced_block_declares() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_unclosed_braced_local.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace Other {\n",
        "    function greet(int $count): void {}\n",
        "}\n",
        "namespace App {\n",
        "    function greet(string $name): void {}\n",
        "    greet(\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 10).await.unwrap();
    assert_eq!(sig_label(&sh), "(string $name): void");
}

#[tokio::test]
async fn qualified_function_expands_through_a_class_import() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_class_import_func.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App\\Helpers {\n",
        "    function greet(int $times): void {}\n",
        "}\n",
        "namespace Lib\\Helpers {\n",
        "    function greet(string $name): void {}\n",
        "}\n",
        "namespace App {\n",
        "    use Lib\\Helpers;\n",
        "    Helpers\\greet();\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 9, 18).await.unwrap();
    assert_eq!(sig_label(&sh), "(string $name): void");
}

#[tokio::test]
async fn qualified_function_from_another_file_through_a_class_import() {
    let backend = create_test_backend();
    let lib_uri = Url::parse("file:///sig_lib_helpers.php").unwrap();
    let lib = concat!(
        "<?php\n",
        "namespace Lib\\Helpers;\n",
        "function greet(string $name, int $times): void {}\n",
    );
    open_php(&backend, &lib_uri, lib).await;

    let uri = Url::parse("file:///sig_app_caller.php").unwrap();
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "use Lib\\Helpers as H;\n",
        "h\\greet('Ada', \n",
    );

    let sh = sig_help_at(&backend, &uri, text, 3, 15).await.unwrap();
    assert_eq!(sig_label(&sh), "(string $name, int $times): void");
    assert_eq!(active_param(&sh), 1);
}

#[tokio::test]
async fn standalone_function_second_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_func2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function greet(string $name, int $age): void {}\n",
        "greet('Alice', \n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 15).await.unwrap();
    assert_eq!(active_param(&sh), 1);
}

#[tokio::test]
async fn standalone_function_no_params() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_noparam.php").unwrap();
    let text = concat!("<?php\n", "function doWork(): void {}\n", "doWork(\n",);

    let sh = sig_help_at(&backend, &uri, text, 2, 7).await.unwrap();
    assert_eq!(sig_label(&sh), "(): void");
    assert!(sh.signatures[0].parameters.as_ref().unwrap().is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Instance method on $this
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn this_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    public function greet(string $name, int $age): string {\n",
        "        return '';\n",
        "    }\n",
        "    public function test() {\n",
        "        $this->greet(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 22).await.unwrap();
    assert!(sig_label(&sh).contains("string $name"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn this_method_second_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_this2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Greeter {\n",
        "    public function greet(string $name, int $age): string {\n",
        "        return '';\n",
        "    }\n",
        "    public function test() {\n",
        "        $this->greet('Alice', \n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 30).await.unwrap();
    assert_eq!(active_param(&sh), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Instance method on a variable
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn variable_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_var.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Calculator {\n",
        "    public function add(int $a, int $b): int { return $a + $b; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $calc = new Calculator();\n",
        "        $calc->add(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 7, 19).await.unwrap();
    assert!(sig_label(&sh).contains("int $a"));
    assert!(sig_label(&sh).contains("int $b"));
    assert_eq!(active_param(&sh), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Static method call
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn static_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MathUtil {\n",
        "    public static function clamp(int $value, int $min, int $max): int {\n",
        "        return max($min, min($max, $value));\n",
        "    }\n",
        "}\n",
        "MathUtil::clamp(\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 16).await.unwrap();
    assert!(sig_label(&sh).contains("int $value"));
    assert_eq!(active_param(&sh), 0);
    let pl = param_labels(&sh);
    assert_eq!(pl.len(), 3);
}

#[tokio::test]
async fn static_method_third_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_static3.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class MathUtil {\n",
        "    public static function clamp(int $value, int $min, int $max): int {\n",
        "        return max($min, min($max, $value));\n",
        "    }\n",
        "}\n",
        "MathUtil::clamp(1, 0, \n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 22).await.unwrap();
    assert_eq!(active_param(&sh), 2);
}

// ═══════════════════════════════════════════════════════════════════════════
//  self:: and static:: calls
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn self_static_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Foo {\n",
        "    public static function create(string $name): static {\n",
        "        return new static();\n",
        "    }\n",
        "    public function test() {\n",
        "        self::create(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 21).await.unwrap();
    assert!(sig_label(&sh).contains("string $name"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Constructor call: new ClassName(
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn constructor_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ctor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function __construct(string $name, string $email) {}\n",
        "}\n",
        "new User(\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 4, 9).await.unwrap();
    assert!(sig_label(&sh).contains("string $name"));
    assert!(sig_label(&sh).contains("string $email"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn constructor_second_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ctor2.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class User {\n",
        "    public function __construct(string $name, string $email) {}\n",
        "}\n",
        "new User('Alice', \n",
    );

    let sh = sig_help_at(&backend, &uri, text, 4, 18).await.unwrap();
    assert_eq!(active_param(&sh), 1);
}

#[tokio::test]
async fn constructor_self_keyword() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ctor_self.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Config {\n",
        "    public function __construct(string $host, int $port) {}\n",
        "    public static function create(): self {\n",
        "        return new self(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 4, 24).await.unwrap();
    assert!(sig_label(&sh).contains("string $host"));
    assert!(sig_label(&sh).contains("int $port"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn constructor_static_keyword() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ctor_static.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Factory {\n",
        "    public function __construct(array $options, bool $debug = false) {}\n",
        "    public static function make(): static {\n",
        "        return new static(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 4, 26).await.unwrap();
    assert!(sig_label(&sh).contains("array $options"));
    assert!(sig_label(&sh).contains("bool $debug"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn constructor_parent_keyword() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ctor_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Animal {\n",
        "    public function __construct(string $name, int $age) {}\n",
        "}\n",
        "class Dog extends Animal {\n",
        "    public function __construct(string $name, int $age, string $breed) {\n",
        "        parent::__construct(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 28).await.unwrap();
    assert!(sig_label(&sh).contains("string $name"));
    assert!(sig_label(&sh).contains("int $age"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn constructor_parent_keyword_with_new() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ctor_new_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function __construct(string $value) {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function test(): void {\n",
        "        $x = new parent(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 24).await.unwrap();
    assert!(sig_label(&sh).contains("string $value"));
    assert_eq!(active_param(&sh), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  No signature help outside parentheses
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn no_help_outside_parens() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_none.php").unwrap();
    let text = concat!("<?php\n", "foo();\n",);

    let sh = sig_help_at(&backend, &uri, text, 1, 6).await;
    assert!(sh.is_none());
}

#[tokio::test]
async fn no_help_on_unknown_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_unknown.php").unwrap();
    let text = concat!("<?php\n", "unknownFunc(\n",);

    let sh = sig_help_at(&backend, &uri, text, 1, 12).await;
    assert!(sh.is_none());
}

// ═══════════════════════════════════════════════════════════════════════════
//  Nested calls — signature help for inner call
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn nested_call_inner_function() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_nested.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function outer(string $x): void {}\n",
        "function inner(int $y, int $z): int { return $y; }\n",
        "outer(inner(\n",
    );

    // Cursor is inside inner(
    let sh = sig_help_at(&backend, &uri, text, 3, 12).await.unwrap();
    assert!(sig_label(&sh).contains("int $y"));
    assert_eq!(active_param(&sh), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Variadic parameter — active index stays on last param
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn variadic_parameter_clamped() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_variadic.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function logMessage(string $level, string ...$parts): void {}\n",
        "logMessage('info', 'a', 'b', \n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 29).await.unwrap();
    // 3 commas → active_parameter = 3, but last param (index 1) is variadic
    // so it should be clamped to 1.
    assert_eq!(active_param(&sh), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Inherited method
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn inherited_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_inherit.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function doWork(int $count): void {}\n",
        "}\n",
        "class Child extends Base {}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $child = new Child();\n",
        "        $child->doWork(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 8, 23).await.unwrap();
    assert!(sig_label(&sh).contains("int $count"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Trait method
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn trait_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_trait.php").unwrap();
    let text = concat!(
        "<?php\n",
        "trait Greetable {\n",
        "    public function greet(string $whom): string { return 'hi'; }\n",
        "}\n",
        "class Person {\n",
        "    use Greetable;\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $p = new Person();\n",
        "        $p->greet(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 10, 18).await.unwrap();
    assert!(sig_label(&sh).contains("string $whom"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Built-in (stub) function
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn stub_function() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///sig_stub.php").unwrap();
    let text = concat!("<?php\n", "str_contains(\n",);

    let sh = sig_help_at(&backend, &uri, text, 1, 13).await.unwrap();
    assert!(sig_label(&sh).contains("$haystack"));
    assert!(sig_label(&sh).contains("$needle"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn stub_function_second_param() {
    let backend = create_test_backend_with_function_stubs();
    let uri = Url::parse("file:///sig_stub2.php").unwrap();
    let text = concat!("<?php\n", "str_contains('hello', \n",);

    let sh = sig_help_at(&backend, &uri, text, 1, 22).await.unwrap();
    assert_eq!(active_param(&sh), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Parameter label offsets are correct substrings
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn parameter_label_offsets_match_label() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_offsets.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function mix(string $a, int $b, bool $c): void {}\n",
        "mix(\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 4).await.unwrap();
    let sig = &sh.signatures[0];
    let params = sig.parameters.as_ref().unwrap();

    for pi in params {
        match &pi.label {
            ParameterLabel::LabelOffsets([start, end]) => {
                let substr = &sig.label[*start as usize..*end as usize];
                // Each extracted label should be a valid parameter representation.
                assert!(
                    substr.contains('$'),
                    "Parameter label offset '{}' should contain a $ sign",
                    substr
                );
            }
            ParameterLabel::Simple(s) => {
                assert!(
                    sig.label.contains(s.as_str()),
                    "Simple label '{}' should be substring of '{}'",
                    s,
                    sig.label
                );
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  String arguments with commas don't confuse parameter counting
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn string_with_commas_ignored() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_strcomma.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function pair(string $a, string $b): void {}\n",
        "pair('a,b,c', \n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 14).await.unwrap();
    // The comma inside the string should not be counted.
    assert_eq!(active_param(&sh), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Nested call arguments don't confuse parameter counting
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn nested_call_args_not_counted() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_nestedcount.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function outer(int $x, int $y): void {}\n",
        "function inner(int $a, int $b): int { return 0; }\n",
        "outer(inner(1, 2), \n",
    );

    let sh = sig_help_at(&backend, &uri, text, 3, 19).await.unwrap();
    assert!(sig_label(&sh).contains("int $x"));
    // inner(1, 2) is one argument to outer, then the comma after it
    // puts us on the second parameter.
    assert_eq!(active_param(&sh), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  parent:: calls
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn parent_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_parent.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Base {\n",
        "    public function __construct(string $name) {}\n",
        "}\n",
        "class Child extends Base {\n",
        "    public function __construct(string $name, int $age) {\n",
        "        parent::__construct(\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 28).await.unwrap();
    assert!(sig_label(&sh).contains("string $name"));
    // The parent __construct only has 1 param ($name).
    let pl = param_labels(&sh);
    assert_eq!(pl.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Cursor right after open paren (no typing yet)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn cursor_right_after_open_paren() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_openparen.php").unwrap();
    let text = concat!("<?php\n", "function test(int $x): void {}\n", "test(",);

    let sh = sig_help_at(&backend, &uri, text, 2, 5).await.unwrap();
    assert!(sig_label(&sh).contains("int $x"));
    assert_eq!(active_param(&sh), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Cursor after comma with spaces
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn cursor_after_comma_with_spaces() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_spaces.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function pair(string $a, string $b): void {}\n",
        "pair('x',   ",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 12).await.unwrap();
    assert_eq!(active_param(&sh), 1);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Cross-file via PSR-4
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn cross_file_psr4_method() {
    let composer_json = r#"{
        "autoload": {
            "psr-4": { "App\\": "src/" }
        }
    }"#;
    let service_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Service {\n",
        "    public function process(string $input, int $retries): bool {\n",
        "        return true;\n",
        "    }\n",
        "}\n",
    );
    let client_php = concat!(
        "<?php\n",
        "namespace App;\n",
        "class Client {\n",
        "    public function run() {\n",
        "        $svc = new Service();\n",
        "        $svc->process(\n",
        "    }\n",
        "}\n",
    );

    let (backend, _dir) = crate::common::create_psr4_workspace(
        composer_json,
        &[
            ("src/Service.php", service_php),
            ("src/Client.php", client_php),
        ],
    );

    let service_uri = Url::from_file_path(_dir.path().join("src/Service.php")).unwrap();
    let client_uri = Url::from_file_path(_dir.path().join("src/Client.php")).unwrap();

    // Open both files
    open_php(&backend, &service_uri, service_php).await;

    open_php(&backend, &client_uri, client_php).await;

    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: client_uri.clone(),
            },
            position: Position {
                line: 5,
                character: 22,
            },
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        context: None,
    };

    let sh = backend.signature_help(params).await.unwrap().unwrap();
    assert!(sig_label(&sh).contains("string $input"));
    assert!(sig_label(&sh).contains("int $retries"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Return type appears in signature label
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn return_type_in_label() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ret.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function compute(int $x): float {}\n",
        "compute(\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 8).await.unwrap();
    assert!(
        sig_label(&sh).ends_with(": float"),
        "Label should end with return type, got: {}",
        sig_label(&sh)
    );
}

#[tokio::test]
async fn no_return_type_shows_mixed() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_noret.php").unwrap();
    let text = concat!("<?php\n", "function doStuff($x) {}\n", "doStuff(\n",);

    let sh = sig_help_at(&backend, &uri, text, 2, 8).await.unwrap();
    assert_eq!(sig_label(&sh), "($x): mixed");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Reference parameter
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn reference_parameter() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_ref.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function swap(int &$a, int &$b): void {}\n",
        "swap(\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 5).await.unwrap();
    let pl = param_labels(&sh);
    assert_eq!(pl[0], "int &$a");
    assert_eq!(pl[1], "int &$b");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Variadic parameter display
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn variadic_parameter_in_label() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_variadic_label.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function collect(string ...$items): array {}\n",
        "collect(\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 2, 8).await.unwrap();
    assert!(
        sig_label(&sh).contains("...$items"),
        "Label should show variadic, got: {}",
        sig_label(&sh)
    );
}

// ═══════════════════════════════════════════════════════════════════════════
//  Cursor in the middle of typing an argument
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn cursor_mid_argument() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_mid.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function pair(string $a, string $b): void {}\n",
        "pair($x",
    );

    // Cursor is at end of `$x` (still first argument)
    let sh = sig_help_at(&backend, &uri, text, 2, 7).await.unwrap();
    assert_eq!(active_param(&sh), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
//  Multiple signatures not applicable (PHP doesn't have overloading)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn single_signature_returned() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_single.php").unwrap();
    let text = concat!("<?php\n", "function doIt(int $n): void {}\n", "doIt(\n",);

    let sh = sig_help_at(&backend, &uri, text, 2, 5).await.unwrap();
    assert_eq!(sh.signatures.len(), 1);
    assert_eq!(sh.active_signature, Some(0));
}

// ═══════════════════════════════════════════════════════════════════════════
//  AST-based chain resolution (property chains, method return chains, etc.)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn property_chain_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_prop_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Inner {\n",
        "    public function process(string $data, int $flags): bool { return true; }\n",
        "}\n",
        "class Outer {\n",
        "    /** @var Inner */\n",
        "    public Inner $inner;\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $outer = new Outer();\n",
        "        $outer->inner->process();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 11, 31).await.unwrap();
    assert!(sig_label(&sh).contains("string $data"));
    assert!(sig_label(&sh).contains("int $flags"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn this_property_chain_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_this_prop_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Service {\n",
        "    public function execute(string $cmd): string { return ''; }\n",
        "}\n",
        "class Controller {\n",
        "    /** @var Service */\n",
        "    public Service $service;\n",
        "    public function run() {\n",
        "        $this->service->execute();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 8, 32).await.unwrap();
    assert!(sig_label(&sh).contains("string $cmd"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn deep_property_chain_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_deep_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Engine {\n",
        "    public function start(int $rpm): bool { return true; }\n",
        "}\n",
        "class Car {\n",
        "    /** @var Engine */\n",
        "    public Engine $engine;\n",
        "}\n",
        "class Garage {\n",
        "    /** @var Car */\n",
        "    public Car $car;\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $garage = new Garage();\n",
        "        $garage->car->engine->start();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 15, 36).await.unwrap();
    assert!(sig_label(&sh).contains("int $rpm"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn method_return_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_method_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where(string $col): self { return $this; }\n",
        "    public function limit(int $n): self { return $this; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $b = new Builder();\n",
        "        $b->where('name')->limit();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 8, 33).await.unwrap();
    assert!(sig_label(&sh).contains("int $n"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn function_return_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_func_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Widget {\n",
        "    public function configure(string $key, string $val): self { return $this; }\n",
        "}\n",
        "/** @return Widget */\n",
        "function makeWidget(): Widget { return new Widget(); }\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        makeWidget()->configure();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 8, 32).await.unwrap();
    assert!(sig_label(&sh).contains("string $key"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn static_method_return_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_static_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Query {\n",
        "    public static function create(): self { return new self(); }\n",
        "    public function filter(string $expr): self { return $this; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        Query::create()->filter();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 7, 32).await.unwrap();
    assert!(sig_label(&sh).contains("string $expr"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn new_expression_chain() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_new_chain.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Printer {\n",
        "    public function print(string $text): void {}\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        (new Printer())->print();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 6, 31).await.unwrap();
    assert!(sig_label(&sh).contains("string $text"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn nullsafe_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_nullsafe.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Formatter {\n",
        "    public function format(string $pattern): string { return ''; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        /** @var Formatter|null $fmt */\n",
        "        $fmt = null;\n",
        "        $fmt?->format();\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 8, 22).await.unwrap();
    assert!(sig_label(&sh).contains("string $pattern"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn property_then_method_chain_second_param() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_chain_2nd.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Logger {\n",
        "    public function log(string $level, string $msg): void {}\n",
        "}\n",
        "class App {\n",
        "    /** @var Logger */\n",
        "    public Logger $logger;\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $app = new App();\n",
        "        $app->logger->log('info', );\n",
        "    }\n",
        "}\n",
    );

    let sh = sig_help_at(&backend, &uri, text, 11, 33).await.unwrap();
    assert!(sig_label(&sh).contains("string $msg"));
    assert_eq!(active_param(&sh), 1);
}

#[tokio::test]
async fn nested_call_correct_site() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_nested_site.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function outer(int $a, int $b): int { return 0; }\n",
        "function inner(string $s): string { return ''; }\n",
        "outer(inner(\n",
    );

    // Cursor inside inner() — should resolve to inner, param 0
    let sh = sig_help_at(&backend, &uri, text, 3, 12).await.unwrap();
    assert!(sig_label(&sh).contains("string $s"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn zero_param_method_closed_parens() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_zero_param.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(): string { return ''; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $pen = new Pen();\n",
        "        $pen->write();\n",
        "    }\n",
        "}\n",
    );
    // Cursor between ( and ) of write() — line 7, char 20
    // "        $pen->write();" — '(' at char 19, ')' at 20
    let result = sig_help_at(&backend, &uri, text, 7, 20).await;
    assert!(
        result.is_some(),
        "signature help should fire for zero-param method"
    );
    let sh = result.unwrap();
    assert!(sig_label(&sh).starts_with("("));
}

#[tokio::test]
async fn constructor_no_explicit_ctor() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_no_ctor.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Simple {\n",
        "    public function greet(): string { return 'hi'; }\n",
        "}\n",
        "$obj = new Simple();\n",
    );
    // Cursor between ( and ) of new Simple() — line 4, char 18
    // "$obj = new Simple();" — '(' at 17, ')' at 18
    let result = sig_help_at(&backend, &uri, text, 4, 18).await;
    assert!(
        result.is_some(),
        "signature help should fire for class with no __construct"
    );
    let sh = result.unwrap();
    assert_eq!(sig_label(&sh), "(): mixed");
}

#[tokio::test]
async fn generic_chain_with_new_expression_arg() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_generic_new_arg.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Product {\n",
        "    public function getPrice(int $qty): float { return 0.0; }\n",
        "}\n",
        "/** @template T */\n",
        "class Wrap {\n",
        "    /** @return T */\n",
        "    public function first(): mixed { return null; }\n",
        "}\n",
        "class Mapper {\n",
        "    /**\n",
        "     * @template T\n",
        "     * @param T $item\n",
        "     * @return Wrap<T>\n",
        "     */\n",
        "    public function wrap(object $item): Wrap { return new Wrap(); }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $mapper = new Mapper();\n",
        "        $mapper->wrap(new Product())->first()->getPrice();\n",
        "    }\n",
        "}\n",
    );

    // Cursor between ( and ) of getPrice() — line 20
    // "        $mapper->wrap(new Product())->first()->getPrice();"
    //  '(' for getPrice is at char 54, ')' at 55
    let sh = sig_help_at(&backend, &uri, text, 20, 56).await.unwrap();
    assert!(sig_label(&sh).contains("int $qty"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn array_access_method_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_array_access.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public function write(string $text): string { return ''; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        /** @var list<Pen> $pens */\n",
        "        $pens = [];\n",
        "        $pens[0]->write();\n",
        "    }\n",
        "}\n",
    );
    // Line 8: "        $pens[0]->write();"
    // '(' at char 23, ')' at 24
    let result = sig_help_at(&backend, &uri, text, 8, 24).await;
    assert!(
        result.is_some(),
        "signature help should fire for array access method call"
    );
    let sh = result.unwrap();
    assert!(sig_label(&sh).starts_with("("));
}

#[tokio::test]
async fn class_string_variable_static_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_class_string.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Pen {\n",
        "    public static function make(string $ink): static { return new static(); }\n",
        "}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $cls = Pen::class;\n",
        "        $cls::make();\n",
        "    }\n",
        "}\n",
    );
    // Line 7: "        $cls::make();"
    // '(' at char 18, ')' at 19
    let sh = sig_help_at(&backend, &uri, text, 7, 19).await.unwrap();
    assert!(sig_label(&sh).contains("string $ink"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn first_class_callable_invocation() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_fcc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function makePen(string $ink): Pen { return new Pen(); }\n",
        "class Pen {}\n",
        "class Demo {\n",
        "    public function test() {\n",
        "        $fn = makePen(...);\n",
        "        $fn();\n",
        "    }\n",
        "}\n",
    );
    // Line 6: "        $fn();"
    // '(' at char 11, ')' at 12
    let sh = sig_help_at(&backend, &uri, text, 6, 12).await.unwrap();
    assert!(sig_label(&sh).contains("string $ink"));
    assert_eq!(active_param(&sh), 0);
}

#[tokio::test]
async fn first_class_callable_invocation_in_enum_method() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_fcc_enum.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function makePen(string $ink): Pen { return new Pen(); }\n",
        "class Pen {}\n",
        "enum Demo {\n",
        "    public function test() {\n",
        "        $fn = makePen(...);\n",
        "        $fn();\n",
        "    }\n",
        "}\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 6, 12)
        .await
        .expect("signature help for the callable");
    assert!(sig_label(&sh).contains("string $ink"));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Default values in parameter labels
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn default_value_in_label() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_default.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function greet(string $name = 'World', int $count = 1): void {}\n",
        "greet();\n",
    );
    // Line 2: "greet();" — cursor inside parens at char 6
    let sh = sig_help_at(&backend, &uri, text, 2, 6).await.unwrap();
    let label = sig_label(&sh);
    assert!(
        label.contains("= 'World'"),
        "Expected default value in label, got: {}",
        label
    );
    assert!(
        label.contains("= 1"),
        "Expected default value in label, got: {}",
        label
    );
    // Verify parameter label offsets match what's in the label string.
    let labels = param_labels(&sh);
    assert_eq!(labels[0], "string $name = 'World'");
    assert_eq!(labels[1], "int $count = 1");
}

#[tokio::test]
async fn required_param_no_default_suffix() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_req.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function process(string $input): void {}\n",
        "process();\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 2, 8).await.unwrap();
    let labels = param_labels(&sh);
    assert_eq!(labels[0], "string $input");
}

#[tokio::test]
async fn method_default_value_in_label() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_meth_def.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    public function fetch(int $page = 1, int $limit = 25): array { return []; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test(Svc $s): void {\n",
        "        $s->fetch();\n",
        "    }\n",
        "}\n",
    );
    // Line 6: "        $s->fetch();" — cursor inside parens
    let sh = sig_help_at(&backend, &uri, text, 6, 18).await.unwrap();
    let labels = param_labels(&sh);
    assert_eq!(labels[0], "int $page = 1");
    assert_eq!(labels[1], "int $limit = 25");
}

// ═══════════════════════════════════════════════════════════════════════════
//  Per-parameter @param descriptions
// ═══════════════════════════════════════════════════════════════════════════

/// Helper to extract parameter documentation strings from a SignatureHelp response.
fn param_docs(sh: &SignatureHelp) -> Vec<Option<String>> {
    let sig = &sh.signatures[0];
    sig.parameters
        .as_ref()
        .unwrap()
        .iter()
        .map(|pi| match &pi.documentation {
            Some(Documentation::MarkupContent(mc)) => Some(mc.value.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn param_description_from_docblock() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_param_doc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * Register a new user.\n",
        " * @param string $name The user's display name.\n",
        " * @param string $email The user's email address.\n",
        " */\n",
        "function register(string $name, string $email): void {}\n",
        "register();\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 7, 9).await.unwrap();
    let docs = param_docs(&sh);
    assert_eq!(docs[0].as_deref(), Some("The user's display name."));
    assert_eq!(docs[1].as_deref(), Some("The user's email address."));
}

#[tokio::test]
async fn param_without_description_has_no_doc() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_param_nodoc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "function simple(int $x): void {}\n",
        "simple();\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 2, 7).await.unwrap();
    let docs = param_docs(&sh);
    assert_eq!(docs[0], None);
}

#[tokio::test]
async fn method_param_description() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_mparam_doc.php").unwrap();
    let text = concat!(
        "<?php\n",
        "class Svc {\n",
        "    /**\n",
        "     * Fetch items from the API.\n",
        "     * @param int $page The page number.\n",
        "     * @param int $limit Max results per page.\n",
        "     */\n",
        "    public function fetch(int $page, int $limit): array { return []; }\n",
        "}\n",
        "class Demo {\n",
        "    public function test(Svc $s): void {\n",
        "        $s->fetch();\n",
        "    }\n",
        "}\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 11, 18).await.unwrap();
    let docs = param_docs(&sh);
    assert_eq!(docs[0].as_deref(), Some("The page number."));
    assert_eq!(docs[1].as_deref(), Some("Max results per page."));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Effective type prefix in param docs
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn param_doc_shows_effective_type_when_different_from_native() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_eff.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param list<string> $items The collected items.\n",
        " */\n",
        "function consume(array $items): void {}\n",
        "consume();\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 5, 8).await.unwrap();

    // Label uses native type.
    assert_eq!(param_labels(&sh), vec!["array $items"]);

    // Doc shows effective type prefix because list<string> != array.
    let docs = param_docs(&sh);
    assert_eq!(
        docs[0].as_deref(),
        Some("`list<string>` The collected items.")
    );
}

#[tokio::test]
async fn param_doc_no_effective_prefix_when_types_match() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_same.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @param string $name The user name.\n",
        " */\n",
        "function greet(string $name): void {}\n",
        "greet();\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 5, 6).await.unwrap();

    // Doc is just the description, no type prefix.
    let docs = param_docs(&sh);
    assert_eq!(docs[0].as_deref(), Some("The user name."));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Combined: defaults + docs together
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn combined_defaults_and_docs() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_combined.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * Paginate results.\n",
        " * @param int $page Current page number.\n",
        " * @param int $size Items per page.\n",
        " * @return array The paginated results.\n",
        " */\n",
        "function paginate(int $page = 1, int $size = 20): array { return []; }\n",
        "paginate();\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 8, 9).await.unwrap();

    // Default values in label
    let labels = param_labels(&sh);
    assert_eq!(labels[0], "int $page = 1");
    assert_eq!(labels[1], "int $size = 20");

    // Parameter docs
    let pdocs = param_docs(&sh);
    assert_eq!(pdocs[0].as_deref(), Some("Current page number."));
    assert_eq!(pdocs[1].as_deref(), Some("Items per page."));

    // Signature-level documentation is always None.
    assert!(sh.signatures[0].documentation.is_none());
}

#[tokio::test]
async fn param_doc_class_string_effective_type() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///sig_class_string.php").unwrap();
    let text = concat!(
        "<?php\n",
        "/**\n",
        " * @template T\n",
        " * @param class-string<T> $class The class name\n",
        " * @return T\n",
        " */\n",
        "function resolve(string $class): object\n",
        "{\n",
        "    return new $class();\n",
        "}\n",
        "resolve();\n",
    );
    let sh = sig_help_at(&backend, &uri, text, 10, 8).await.unwrap();

    // Label uses native type for params, effective for return.
    assert_eq!(sig_label(&sh), "(string $class): T");

    // Doc shows effective type because class-string<T> != string.
    let docs = param_docs(&sh);
    assert_eq!(docs[0].as_deref(), Some("`class-string<T>` The class name"));
}

// ─── Scope method signature help on Builder instances ───────────────────────

/// Reproduces the Builder cache-poisoning scenario for signature help.
///
/// When the resolved_class_cache is seeded with a plain Builder entry
/// (no model-specific scope methods) by resolving a different model's
/// Builder chain first, subsequent signature help for a scope method
/// on a *different* model's Builder chain must still work.
///
/// This mirrors the hover cache-poisoning test
/// (`hover_scope_survives_builder_cache_poisoning`) but exercises the
/// `resolve_instance_method_callable` path used by signature help.
#[tokio::test]
async fn sig_help_scope_survives_builder_cache_poisoning() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test.php").unwrap();
    let content = r#"<?php
namespace Illuminate\Database\Eloquent {
    abstract class Model {
        /** @return \Illuminate\Database\Eloquent\Builder<static> */
        public static function query() {}
    }

    /**
     * @template TModel of \Illuminate\Database\Eloquent\Model
     * @mixin \Illuminate\Database\Query\Builder
     */
    class Builder {
        /** @return $this */
        public function where($column, $operator = null, $value = null) {}
        /** @return \Illuminate\Database\Eloquent\Collection<int, TModel> */
        public function get($columns = ['*']) { return new Collection(); }
    }

    /** @template TKey @template TModel */
    class Collection {}
}

namespace Illuminate\Database\Query {
    class Builder {
        /** @return $this */
        public function limit(int $value) { return $this; }
    }
}

namespace App {
    use Illuminate\Database\Eloquent\Model;
    use Illuminate\Database\Eloquent\Builder;

    // Model with NO scope methods — resolving its Builder chain seeds
    // the cache with a plain Builder entry.
    class PlainModel extends Model {}

    // Model WITH a scope method that takes a parameter.
    class ScopedModel extends Model {
        public function scopeOfCategory(Builder $query, string $category): void {}
    }

    class Demo {
        public function run(): void {
            // Step 1: trigger Builder resolution for PlainModel (no scopes).
            PlainModel::where('id', 1)->get();

            // Step 2: signature help inside ofCategory() must resolve
            // even though the Builder cache was seeded without scopes.
            ScopedModel::where('active', 1)->ofCategory('electronics');
        }
    }
}
"#;

    // Open the file (triggers update_ast + diagnostics, populating caches).
    open_php(&backend, &uri, content).await;

    // ── Step 1: Signature help on get() on PlainModel to seed Builder cache ──
    // We trigger signature help on get() first so the Builder FQN is
    // cached without ScopedModel's scope methods.
    let lines: Vec<&str> = content.lines().collect();
    let get_line_idx = lines
        .iter()
        .enumerate()
        .find(|(_, l)| l.contains("PlainModel::where('id', 1)->get("))
        .map(|(i, _)| i)
        .expect("should find PlainModel get() line");
    let get_line = lines[get_line_idx];
    // Place cursor inside `get(` parens
    let get_col = get_line.find("get(").expect("should find get(") as u32 + 4;
    let sh_get = backend
        .signature_help(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: get_line_idx as u32,
                    character: get_col,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        })
        .await
        .unwrap();
    assert!(
        sh_get.is_some(),
        "signature help should work on get() after PlainModel::where() (line {})",
        get_line_idx
    );

    // ── Step 2: Signature help on ofCategory() after ScopedModel::where() ──
    let scope_line_idx = lines
        .iter()
        .enumerate()
        .find(|(_, l)| l.contains("->ofCategory('electronics')"))
        .map(|(i, _)| i)
        .expect("should find ofCategory line");
    let scope_line = lines[scope_line_idx];
    // Place cursor inside `ofCategory(` after the opening paren
    let scope_col = scope_line
        .find("ofCategory(")
        .expect("should find ofCategory(") as u32
        + 11;
    let sh_scope = backend
        .signature_help(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position {
                    line: scope_line_idx as u32,
                    character: scope_col,
                },
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            context: None,
        })
        .await
        .unwrap();
    assert!(
        sh_scope.is_some(),
        "signature help should work on ofCategory() after ScopedModel::where() even when Builder cache was seeded by PlainModel (line {})",
        scope_line_idx
    );

    // The scope method `scopeOfCategory(Builder $query, string $category)`
    // is exposed as `ofCategory(string $category)` (first Builder param
    // stripped).  Verify the signature mentions the parameter.
    let sh = sh_scope.unwrap();
    let label = &sh.signatures[0].label;
    assert!(
        label.contains("category"),
        "signature label should mention 'category' param, got: {}",
        label
    );
}

// ─── Signature help suppression inside closure bodies ───────────────────────

/// Signature help for the outer `where()` call should NOT fire when the
/// cursor is inside the closure body `{ ... }` that is an argument.
///
/// The closure signature is still useful while typing the closure's
/// parameter list (before `{`), but once you are inside the braces you
/// are writing code in the context of the closure, not `where()`.
#[tokio::test]
async fn no_help_inside_closure_body_argument() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/sig_closure_body.php").unwrap();

    // Line 0:  <?php
    // Line 1:  class Builder {
    // Line 2:      /**
    // Line 3:       * @param (\Closure(static): mixed)|string|array $column
    // Line 4:       * @return static
    // Line 5:       */
    // Line 6:      public function where($column, $operator = null, $value = null): static { return $this; }
    // Line 7:      public function orWhere(string $col, mixed $val = null): static { return $this; }
    // Line 8:  }
    // Line 9:  $b = new Builder();
    // Line 10: $b->where(function ($q): void {
    // Line 11:     $q->orWhere('domain', 'test');
    // Line 12: });
    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "}\n",
        "$b = new Builder();\n",
        "$b->where(function ($q): void {\n",
        "    $q->orWhere('domain', 'test');\n",
        "});\n",
    );

    // Inside the closure body (line 11, within the statement) — should NOT
    // show signature help for the outer `where()`.
    let result = sig_help_at(&backend, &uri, src, 11, 4).await;
    assert!(
        result.is_none(),
        "Expected no signature help inside closure body, got: {:?}",
        result,
    );
}

/// Signature help should still fire when the cursor is in the closure's
/// parameter list (before `{`), because you are still filling in the
/// argument to `where()` at that point.
#[tokio::test]
async fn help_still_fires_in_closure_param_list() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/sig_closure_params.php").unwrap();

    // Line 0:  <?php
    // Line 1:  class Builder {
    // Line 2:      /**
    // Line 3:       * @param (\Closure(static): mixed)|string|array $column
    // Line 4:       * @return static
    // Line 5:       */
    // Line 6:      public function where($column, $operator = null): static { return $this; }
    // Line 7:  }
    // Line 8:  $b = new Builder();
    // Line 9:  $b->where(function ($q): void {
    // Line 10:     $q->where('x');
    // Line 11: });
    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null): static { return $this; }\n",
        "}\n",
        "$b = new Builder();\n",
        "$b->where(function ($q): void {\n",
        "    $q->where('x');\n",
        "});\n",
    );

    // Cursor right after `where(` on line 9, before `function` — still
    // inside the call's argument list but NOT inside the closure body.
    let result = sig_help_at(&backend, &uri, src, 9, 11).await;
    assert!(
        result.is_some(),
        "Expected signature help for where() before the closure body starts",
    );
    let sh = result.unwrap();
    assert!(
        sig_label(&sh).contains("$column"),
        "Expected where()'s $column param in label, got: {}",
        sig_label(&sh),
    );
}

/// When there is a nested call *inside* the closure body, signature help
/// should fire for that inner call, not the outer one.
#[tokio::test]
async fn inner_call_inside_closure_body_shows_inner_sig() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/sig_closure_inner.php").unwrap();

    // Line 0:  <?php
    // Line 1:  class Builder {
    // Line 2:      /**
    // Line 3:       * @param (\Closure(static): mixed)|string|array $column
    // Line 4:       * @return static
    // Line 5:       */
    // Line 6:      public function where($column, $operator = null, $value = null): static { return $this; }
    // Line 7:      public function orWhere(string $col, mixed $val = null): static { return $this; }
    // Line 8:  }
    // Line 9:  $b = new Builder();
    // Line 10: $b->where(function ($q): void {
    // Line 11:     $q->orWhere('domain',
    // Line 12: });
    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    /**\n",
        "     * @param (\\Closure(static): mixed)|string|array $column\n",
        "     * @return static\n",
        "     */\n",
        "    public function where($column, $operator = null, $value = null): static { return $this; }\n",
        "    public function orWhere(string $col, mixed $val = null): static { return $this; }\n",
        "}\n",
        "$b = new Builder();\n",
        "$b->where(function ($q): void {\n",
        "    $q->orWhere('domain', );\n",
        "});\n",
    );

    // Cursor at line 11 col 26, after the comma in `orWhere('domain', )` —
    // should show signature help for orWhere, not where.
    let result = sig_help_at(&backend, &uri, src, 11, 26).await;
    assert!(
        result.is_some(),
        "Expected signature help for inner orWhere() call inside closure body",
    );
    let sh = result.unwrap();
    let label = sig_label(&sh);
    assert!(
        label.contains("$col") || label.contains("$val"),
        "Expected orWhere()'s params in label (not where()'s), got: {}",
        label,
    );
    // Make sure it's NOT showing where()'s signature.
    assert!(
        !label.contains("$column"),
        "Should NOT show outer where()'s $column param, got: {}",
        label,
    );
}

/// Signature help suppression also works when the closure uses `use ($var)`.
#[tokio::test]
async fn no_help_inside_closure_body_with_use_clause() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/sig_closure_use.php").unwrap();

    let src = concat!(
        "<?php\n",
        "class Builder {\n",
        "    public function where($column, $operator = null): static { return $this; }\n",
        "}\n",
        "$search = 'test';\n",
        "$b = new Builder();\n",
        "$b->where(function ($q) use ($search): void {\n",
        "    $q->where('domain', 'like', '%' . $search . '%');\n",
        "});\n",
    );

    // Line 7: inside the closure body statement — no sig help for outer where().
    let result = sig_help_at(&backend, &uri, src, 7, 8).await;
    assert!(
        result.is_none(),
        "Expected no signature help inside closure body (with use clause), got: {:?}",
        result,
    );
}

/// Signature help suppression works inside arrow function bodies too.
#[tokio::test]
async fn no_help_inside_arrow_fn_body_argument() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/sig_arrow_fn_body.php").unwrap();

    // Line 0:  <?php
    // Line 1:  function array_map(callable $callback, array $array): array { return []; }
    // Line 2:  function strtoupper(string $s): string { return $s; }
    // Line 3:  $names = ['alice', 'bob'];
    // Line 4:  array_map(fn($n) => strtoupper($n), $names);
    let src = concat!(
        "<?php\n",
        "function array_map(callable $callback, array $array): array { return []; }\n",
        "function strtoupper(string $s): string { return $s; }\n",
        "$names = ['alice', 'bob'];\n",
        "array_map(fn($n) => strtoupper($n), $names);\n",
    );

    // Cursor inside `strtoupper($n)` on line 4 — should show strtoupper's
    // sig, not array_map's.
    let result = sig_help_at(&backend, &uri, src, 4, 33).await;
    // Either no result or the inner strtoupper result — NOT array_map's.
    if let Some(sh) = result {
        let label = sig_label(&sh);
        assert!(
            !label.contains("$callback"),
            "Should NOT show outer array_map()'s $callback param inside arrow fn body, got: {}",
            label,
        );
    }
}

/// Arrow function body with no inner call — the exact pattern from the
/// real-world `$this->ratings->each(fn(Model $m): float => $m->percentage = ...)`.
///
/// Signature help for `each()` should NOT fire when the cursor is past
/// the `=>` in the arrow function body.
#[tokio::test]
async fn no_help_inside_arrow_fn_body_no_inner_call() {
    let backend = create_test_backend();
    let uri = Url::parse("file:///test/sig_arrow_fn_no_inner.php").unwrap();

    // Line 0:  <?php
    // Line 1:  class Collection {
    // Line 2:      /** @param callable(mixed): mixed $callback */
    // Line 3:      public function each(callable $callback): static { return $this; }
    // Line 4:  }
    // Line 5:  class ReviewModel {
    // Line 6:      public float $percentage = 0.0;
    // Line 7:      public int $count = 0;
    // Line 8:  }
    // Line 9:  $ratings = new Collection();
    // Line 10: $total = 10;
    // Line 11: $ratings->each(fn(ReviewModel $model): float => $model->percentage = $total > 0 ? ($model->count / $total) * 100 : 0.0);
    let src = concat!(
        "<?php\n",
        "class Collection {\n",
        "    /** @param callable(mixed): mixed $callback */\n",
        "    public function each(callable $callback): static { return $this; }\n",
        "}\n",
        "class ReviewModel {\n",
        "    public float $percentage = 0.0;\n",
        "    public int $count = 0;\n",
        "}\n",
        "$ratings = new Collection();\n",
        "$total = 10;\n",
        "$ratings->each(fn(ReviewModel $model): float => $model->percentage = $total > 0 ? ($model->count / $total) * 100 : 0.0);\n",
    );

    // Cursor inside the arrow fn body, on `$model->percentage` (line 11, col 60).
    // Should NOT show signature help for each().
    let result = sig_help_at(&backend, &uri, src, 11, 60).await;
    assert!(
        result.is_none(),
        "Expected no signature help inside arrow fn body (no inner call), got: {:?}",
        result,
    );

    // Cursor right after `each(` but before `fn` (line 11, col 16) — should
    // still show signature help for each().
    let result = sig_help_at(&backend, &uri, src, 11, 16).await;
    assert!(
        result.is_some(),
        "Expected signature help for each() before the arrow fn starts",
    );
}
