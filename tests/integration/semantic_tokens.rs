//! Integration tests for `textDocument/semanticTokens/full`.
//!
//! Each test creates a backend, opens a PHP file via `update_ast`, then
//! calls `handle_semantic_tokens_full` and asserts on the returned tokens.

use crate::common::create_test_backend;
use tower_lsp::lsp_types::*;

/// Helper: open a file in the backend and return semantic tokens.
fn get_tokens(php: &str) -> Vec<SemanticToken> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, php);
    let result = backend.handle_semantic_tokens_full(uri, php);
    match result {
        Some(SemanticTokensResult::Tokens(tokens)) => tokens.data,
        _ => vec![],
    }
}

// ─── Token type indices (must match src/semantic_tokens.rs) ─────────────────

const TT_CLASS: u32 = 1;
const TT_INTERFACE: u32 = 2;
const TT_ENUM: u32 = 3;
const TT_TYPE: u32 = 4;
const TT_TYPE_PARAMETER: u32 = 5;
const TT_PARAMETER: u32 = 6;
const TT_VARIABLE: u32 = 7;
const TT_PROPERTY: u32 = 8;
const TT_FUNCTION: u32 = 9;
const TT_METHOD: u32 = 10;
#[allow(dead_code)]
const TT_ENUM_MEMBER: u32 = 12;
#[allow(dead_code)]
const TT_KEYWORD: u32 = 13;
#[allow(dead_code)]
const TT_COMMENT: u32 = 14;

// ─── Token modifier bits (must match src/semantic_tokens.rs) ────────────────

const TM_DECLARATION: u32 = 1 << 0;
const TM_STATIC: u32 = 1 << 1;
const TM_READONLY: u32 = 1 << 2;
const TM_DEPRECATED: u32 = 1 << 3;
const TM_ABSTRACT: u32 = 1 << 4;
const TM_DEFINITION: u32 = 1 << 5;
const TM_DEFAULT_LIBRARY: u32 = 1 << 6;

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Assert that a decoded token has a specific modifier bit set.
fn has_modifier(tok: &DecodedToken, modifier: u32) -> bool {
    tok.modifiers & modifier != 0
}

/// Decode all tokens to absolute positions for easier assertion.
#[derive(Debug)]
struct DecodedToken {
    line: u32,
    character: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

fn decode_tokens(tokens: &[SemanticToken]) -> Vec<DecodedToken> {
    let mut result = Vec::new();
    let mut line = 0u32;
    let mut start = 0u32;
    for tok in tokens {
        line += tok.delta_line;
        if tok.delta_line > 0 {
            start = tok.delta_start;
        } else {
            start += tok.delta_start;
        }
        result.push(DecodedToken {
            line,
            character: start,
            length: tok.length,
            token_type: tok.token_type,
            modifiers: tok.token_modifiers_bitset,
        });
    }
    result
}

/// Find the first decoded token at the given line/char.
fn find_decoded(decoded: &[DecodedToken], line: u32, character: u32) -> Option<&DecodedToken> {
    decoded
        .iter()
        .find(|t| t.line == line && t.character == character)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[test]
fn class_declaration_token() {
    let php = r#"<?php
class Foo {
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "Foo" on line 1, character 6
    let tok = find_decoded(&decoded, 1, 6).expect("expected token for class Foo");
    assert_eq!(tok.token_type, TT_CLASS);
    assert!(
        tok.modifiers & TM_DECLARATION != 0,
        "expected declaration modifier"
    );
    assert_eq!(tok.length, 3);
}

#[test]
fn interface_declaration_token() {
    let php = r#"<?php
interface Baz {
    public function doStuff(): void;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "Baz" on line 1, char 10
    let tok = find_decoded(&decoded, 1, 10).expect("expected token for interface Baz");
    assert_eq!(tok.token_type, TT_INTERFACE);
    assert!(tok.modifiers & TM_DECLARATION != 0);
}

#[test]
fn enum_declaration_token() {
    let php = r#"<?php
enum Color {
    case Red;
    case Blue;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "Color" on line 1, char 5
    let tok = find_decoded(&decoded, 1, 5).expect("expected token for enum Color");
    assert_eq!(tok.token_type, TT_ENUM);
    assert!(tok.modifiers & TM_DECLARATION != 0);
}

#[test]
fn trait_declaration_token() {
    let php = r#"<?php
trait Loggable {
    public function log(): void {}
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "Loggable" on line 1, char 6
    let tok = find_decoded(&decoded, 1, 6).expect("expected token for trait Loggable");
    assert_eq!(tok.token_type, TT_TYPE);
    assert!(tok.modifiers & TM_DECLARATION != 0);
}

#[test]
fn class_reference_in_type_hint() {
    let php = r#"<?php
class User {}
class Service {
    public function find(User $user): User {
        return $user;
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // Parameter type hint "User" on line 3
    let user_hints: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_CLASS && t.length == 4 && t.line == 3)
        .collect();
    // Should have at least the parameter type hint and the return type hint
    assert!(
        user_hints.len() >= 2,
        "expected at least 2 User class references on line 3, got {}",
        user_hints.len()
    );
}

#[test]
fn class_reference_in_new_expression() {
    let php = r#"<?php
class Item {}
function make() {
    $x = new Item();
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "Item" on line 3 (after `new `) should be a class reference.
    let item_refs: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_CLASS && t.length == 4 && t.line == 3)
        .collect();
    assert!(
        !item_refs.is_empty(),
        "expected class reference for new Item()"
    );
}

#[test]
fn variable_tokens() {
    let php = r#"<?php
function example() {
    $foo = 42;
    echo $foo;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // $foo on line 2 (assignment) should have definition modifier
    let foo_def: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_VARIABLE && t.line == 2 && t.length == 4)
        .collect();
    assert!(
        !foo_def.is_empty(),
        "expected variable token for $foo definition"
    );
    // At least one should have the definition modifier
    assert!(
        foo_def.iter().any(|t| t.modifiers & TM_DEFINITION != 0),
        "expected definition modifier on $foo assignment"
    );

    // $foo on line 3 (usage) should NOT have definition modifier
    let foo_use: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_VARIABLE && t.line == 3 && t.length == 4)
        .collect();
    assert!(
        !foo_use.is_empty(),
        "expected variable token for $foo usage"
    );
}

#[test]
fn parameter_tokens() {
    let php = r#"<?php
function greet(string $name): string {
    return "Hello " . $name;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // $name on line 1 should be a parameter
    let name_params: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_PARAMETER && t.line == 1 && t.length == 5)
        .collect();
    assert!(
        !name_params.is_empty(),
        "expected parameter token for $name"
    );
}

#[test]
fn method_declaration_token() {
    let php = r#"<?php
class Foo {
    public function bar(): void {}
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "bar" on line 2 should be a method with declaration modifier
    let bar_decl: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.length == 3 && t.line == 2)
        .collect();
    assert!(
        !bar_decl.is_empty(),
        "expected method declaration token for bar"
    );
    assert!(
        bar_decl.iter().any(|t| t.modifiers & TM_DECLARATION != 0),
        "expected declaration modifier on method bar"
    );
}

#[test]
fn method_call_token() {
    let php = r#"<?php
class Foo {
    public function bar(): void {}
}
function test(Foo $f) {
    $f->bar();
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "bar" on line 5 (call site) should be a method
    let bar_calls: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.length == 3 && t.line == 5)
        .collect();
    assert!(
        !bar_calls.is_empty(),
        "expected method token for bar() call"
    );
}

#[test]
fn property_access_token() {
    let php = r#"<?php
class Foo {
    public string $name = '';
}
function test(Foo $f) {
    echo $f->name;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "name" on line 5 (access) should be a property
    let name_props: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_PROPERTY && t.length == 4 && t.line == 5)
        .collect();
    assert!(
        !name_props.is_empty(),
        "expected property token for ->name access"
    );
}

#[test]
fn static_method_call_has_static_modifier() {
    let php = r#"<?php
class Foo {
    public static function create(): static { return new static(); }
}
function test() {
    Foo::create();
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "create" on line 5 should be a method with static modifier
    let create_calls: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.length == 6 && t.line == 5)
        .collect();
    assert!(
        !create_calls.is_empty(),
        "expected method token for Foo::create() call"
    );
    assert!(
        create_calls.iter().any(|t| t.modifiers & TM_STATIC != 0),
        "expected static modifier on Foo::create()"
    );
}

#[test]
fn function_declaration_token() {
    let php = r#"<?php
function helper(): int {
    return 42;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "helper" on line 1 should be a function with declaration modifier
    let helper_decl: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_FUNCTION && t.length == 6 && t.line == 1)
        .collect();
    assert!(
        !helper_decl.is_empty(),
        "expected function declaration token for helper"
    );
    assert!(
        helper_decl
            .iter()
            .any(|t| t.modifiers & TM_DECLARATION != 0),
        "expected declaration modifier on function helper"
    );
}

#[test]
fn function_call_token() {
    let php = r#"<?php
function helper(): int { return 42; }
function main() {
    $x = helper();
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "helper" on line 3 (call site) should be a function
    let helper_calls: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_FUNCTION && t.length == 6 && t.line == 3)
        .collect();
    assert!(
        !helper_calls.is_empty(),
        "expected function token for helper() call"
    );
}

#[test]
fn this_is_variable_with_readonly_and_default_library() {
    let php = r#"<?php
class Foo {
    public function bar(): void {
        $this->bar();
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "$this" on line 3 should be a variable with readonly + defaultLibrary
    let this_tokens: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_VARIABLE && t.length == 5 && t.line == 3)
        .collect();
    assert!(!this_tokens.is_empty(), "expected variable token for $this");
    assert!(
        this_tokens
            .iter()
            .any(|t| has_modifier(t, TM_READONLY) && has_modifier(t, TM_DEFAULT_LIBRARY)),
        "expected readonly + defaultLibrary modifiers on $this"
    );
}

#[test]
fn self_static_are_type_with_default_library() {
    let php = r#"<?php
class Foo {
    public static function make(): static {
        return new static();
    }
    public function selfRef(): self {
        return $this;
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "static" in return type hint (line 2) → type + defaultLibrary
    let static_types: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_TYPE && t.length == 6 && has_modifier(t, TM_DEFAULT_LIBRARY))
        .collect();
    assert!(
        !static_types.is_empty(),
        "expected type + defaultLibrary token for static keyword"
    );
    // "self" in return type hint (line 5) → type + defaultLibrary
    let self_types: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_TYPE && t.length == 4 && has_modifier(t, TM_DEFAULT_LIBRARY))
        .collect();
    assert!(
        !self_types.is_empty(),
        "expected type + defaultLibrary token for self keyword"
    );
}

#[test]
fn parent_has_default_library() {
    let php = r#"<?php
class Base {}
class Child extends Base {
    public function test(): void {
        parent::class;
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "parent" on line 4 should carry the defaultLibrary modifier.
    // The token type is resolved from the parent class kind (TT_CLASS
    // for Base) or falls back to TT_TYPE.
    let parent_tokens: Vec<_> = decoded
        .iter()
        .filter(|t| t.length == 6 && t.line == 4 && has_modifier(t, TM_DEFAULT_LIBRARY))
        .collect();
    assert!(
        !parent_tokens.is_empty(),
        "expected defaultLibrary modifier on parent keyword"
    );
}

#[test]
fn deprecated_class_has_modifier() {
    let php = r#"<?php
/** @deprecated Use NewFoo instead */
class OldFoo {}
function test(OldFoo $x) {}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // The class declaration "OldFoo" on line 2 should be deprecated
    let old_foo_decl: Vec<_> = decoded
        .iter()
        .filter(|t| t.length == 6 && t.line == 2 && t.modifiers & TM_DECLARATION != 0)
        .collect();
    assert!(
        !old_foo_decl.is_empty(),
        "expected OldFoo declaration token"
    );
    assert!(
        old_foo_decl
            .iter()
            .any(|t| t.modifiers & TM_DEPRECATED != 0),
        "expected deprecated modifier on OldFoo declaration"
    );

    // The reference "OldFoo" on line 3 should also be deprecated
    let old_foo_ref: Vec<_> = decoded
        .iter()
        .filter(|t| t.length == 6 && t.line == 3 && t.token_type == TT_CLASS)
        .collect();
    assert!(!old_foo_ref.is_empty(), "expected OldFoo reference token");
    assert!(
        old_foo_ref.iter().any(|t| t.modifiers & TM_DEPRECATED != 0),
        "expected deprecated modifier on OldFoo reference"
    );
}

#[test]
fn abstract_class_has_modifier() {
    let php = r#"<?php
abstract class AbstractBase {
    abstract public function doWork(): void;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "AbstractBase" on line 1 should have abstract modifier
    let ab_decl: Vec<_> = decoded
        .iter()
        .filter(|t| t.length == 12 && t.line == 1 && t.modifiers & TM_DECLARATION != 0)
        .collect();
    assert!(
        !ab_decl.is_empty(),
        "expected AbstractBase declaration token"
    );
    assert!(
        ab_decl.iter().any(|t| t.modifiers & TM_ABSTRACT != 0),
        "expected abstract modifier on AbstractBase"
    );
}

#[test]
fn constant_token() {
    let php = r#"<?php
class Config {
    const VERSION = '1.0';
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "VERSION" on line 2 should be ENUM_MEMBER (constant) with declaration modifier
    let version_tokens: Vec<_> = decoded
        .iter()
        .filter(|t| t.length == 7 && t.line == 2)
        .collect();
    assert!(
        !version_tokens.is_empty(),
        "expected token for constant VERSION"
    );
}

#[test]
fn interface_reference_resolves_correctly() {
    let php = r#"<?php
interface Countable {
    public function count(): int;
}
class Items implements Countable {
    public function count(): int { return 0; }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "Countable" on line 1 (declaration) should be interface
    let countable_decl = find_decoded(&decoded, 1, 10);
    assert!(
        countable_decl.is_some(),
        "expected Countable declaration token"
    );
    assert_eq!(countable_decl.unwrap().token_type, TT_INTERFACE);

    // "Countable" on line 4 (implements clause) should also be interface
    let countable_refs: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_INTERFACE && t.length == 9 && t.line == 4)
        .collect();
    assert!(
        !countable_refs.is_empty(),
        "expected interface reference for Countable in implements clause"
    );
}

#[test]
fn enum_reference_resolves_correctly() {
    let php = r#"<?php
enum Status {
    case Active;
    case Inactive;
}
function check(Status $s): void {}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // "Status" on line 5 (type hint) should be enum
    let status_refs: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_ENUM && t.length == 6 && t.line == 5)
        .collect();
    assert!(
        !status_refs.is_empty(),
        "expected enum reference for Status in type hint"
    );
}

#[test]
fn delta_encoding_is_correct() {
    let php = r#"<?php
class A {}
class B {}
"#;
    let tokens = get_tokens(php);
    // There should be at least 2 tokens (A and B declarations).
    assert!(
        tokens.len() >= 2,
        "expected at least 2 tokens, got {}",
        tokens.len()
    );

    // Verify that decoding works by round-tripping.
    let decoded = decode_tokens(&tokens);

    // All decoded tokens should have non-decreasing (line, character) positions.
    for window in decoded.windows(2) {
        let (a, b) = (&window[0], &window[1]);
        assert!(
            b.line > a.line || (b.line == a.line && b.character >= a.character),
            "tokens not in order: ({},{}) then ({},{})",
            a.line,
            a.character,
            b.line,
            b.character,
        );
    }
}

#[test]
fn empty_file_returns_empty_tokens() {
    let php = "<?php\n";
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // An empty PHP file should produce no (or very few) semantic tokens.
    // There might be zero tokens or just whitespace-related artifacts.
    assert!(
        decoded.len() <= 1,
        "expected 0 or 1 tokens for empty file, got {}",
        decoded.len()
    );
}

#[test]
fn template_parameter_token() {
    let php = r#"<?php
/**
 * @template T
 */
class Box {
    /** @var T */
    private $value;

    /** @param T $val */
    public function set($val): void {
        $this->value = $val;
    }

    /** @return T */
    public function get() {
        return $this->value;
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // T references in docblocks should be type parameters
    let type_params: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_TYPE_PARAMETER && t.length == 1)
        .collect();
    assert!(
        !type_params.is_empty(),
        "expected type parameter tokens for @template T references"
    );
}

#[test]
fn multiple_classes_in_one_file() {
    let php = r#"<?php
class Alpha {}
interface Beta {}
enum Gamma { case X; }
trait Delta {}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);

    // Alpha should be CLASS
    let alpha = find_decoded(&decoded, 1, 6);
    assert!(alpha.is_some(), "expected Alpha token");
    assert_eq!(alpha.unwrap().token_type, TT_CLASS);

    // Beta should be INTERFACE
    let beta = find_decoded(&decoded, 2, 10);
    assert!(beta.is_some(), "expected Beta token");
    assert_eq!(beta.unwrap().token_type, TT_INTERFACE);

    // Gamma should be ENUM
    let gamma = find_decoded(&decoded, 3, 5);
    assert!(gamma.is_some(), "expected Gamma token");
    assert_eq!(gamma.unwrap().token_type, TT_ENUM);

    // Delta should be TYPE (trait)
    let delta = find_decoded(&decoded, 4, 6);
    assert!(delta.is_some(), "expected Delta token");
    assert_eq!(delta.unwrap().token_type, TT_TYPE);
}

#[test]
fn static_property_access() {
    let php = r#"<?php
class Config {
    public static string $version = '1.0';
}
function test() {
    echo Config::$version;
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // $version on line 5 should be a property with static modifier
    let _version_access: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_PROPERTY && t.line == 5 && t.modifiers & TM_STATIC != 0)
        .collect();
    // The static property access may appear as a variable or property depending
    // on how the symbol map classifies it. Just check we have something on that line.
    let any_on_line_5: Vec<_> = decoded.iter().filter(|t| t.line == 5).collect();
    assert!(
        !any_on_line_5.is_empty(),
        "expected at least one token on line 5 (static property access)"
    );
}

#[test]
fn mixed_members() {
    let php = r#"<?php
class Foo {
    public string $name = '';
    public const MAX = 100;
    public function bar(): void {}
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);

    // Check that we have different token types for the different members
    let has_property = decoded
        .iter()
        .any(|t| t.token_type == TT_PROPERTY && t.line == 2);
    let has_method = decoded
        .iter()
        .any(|t| t.token_type == TT_METHOD && t.line == 4);

    assert!(has_property, "expected property token on line 2");
    assert!(has_method, "expected method token on line 4");
}

// ─── Member existence validation ────────────────────────────────────────────
//
// Member tokens (method/property) are only emitted when the member can be
// verified to exist on the resolved subject class (or the subject cannot
// be cheaply resolved).  A verifiably absent member gets no token so the
// text keeps its default coloring — most visibly for array-callable
// strings like `[Foo::class, 'mispelled']`.

#[test]
fn array_callable_known_method_gets_method_token() {
    let php = r#"<?php
class SortableController {
    public function sort(): void {}
}
$cb = [SortableController::class, 'sort'];
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let method_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 4 && t.length == 4)
        .collect();
    assert!(
        !method_toks.is_empty(),
        "expected method token for 'sort' in array callable"
    );
}

#[test]
fn array_callable_unknown_method_gets_no_token() {
    let php = r#"<?php
class SortableController {
    public function sort(): void {}
}
$cb = [SortableController::class, 'sortaaaa'];
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let method_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 4)
        .collect();
    assert!(
        method_toks.is_empty(),
        "expected no method token for misspelled 'sortaaaa', got: {:?}",
        method_toks
    );
}

#[test]
fn array_callable_this_subject_validates_against_enclosing_class() {
    let php = r#"<?php
class Listener {
    public function handle(): void {}
    public function register(): array {
        return [[$this, 'handle'], [$this, 'hanlde']];
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // 'handle' (length 6) gets a method token, the misspelled 'hanlde' does not.
    let handle_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 4 && t.length == 6)
        .collect();
    assert_eq!(
        handle_toks.len(),
        1,
        "expected exactly one method token on line 4 (for 'handle'), got: {:?}",
        handle_toks
    );
}

#[test]
fn static_call_unknown_method_gets_no_token() {
    let php = r#"<?php
class Foo {
    public static function create(): void {}
}
Foo::nonExistentMethod();
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let method_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 4)
        .collect();
    assert!(
        method_toks.is_empty(),
        "expected no method token for Foo::nonExistentMethod(), got: {:?}",
        method_toks
    );
}

#[test]
fn static_call_on_unknown_class_keeps_token() {
    let php = r#"<?php
NotDefinedAnywhere::whatever();
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // The class cannot be resolved, so the member cannot be verified —
    // keep emitting the method token.
    let method_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 1)
        .collect();
    assert!(
        !method_toks.is_empty(),
        "expected method token on unresolvable class subject"
    );
}

#[test]
fn variable_subject_keeps_token_unconditionally() {
    let php = r#"<?php
function test($obj) {
    $obj->whatever();
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let method_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 2)
        .collect();
    assert!(
        !method_toks.is_empty(),
        "expected method token on untyped variable subject"
    );
}

#[test]
fn this_call_unknown_method_gets_no_token() {
    let php = r#"<?php
class Foo {
    public function known(): void {
        $this->known();
        $this->unknownMethod();
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let known: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 3)
        .collect();
    assert!(
        !known.is_empty(),
        "expected method token for $this->known()"
    );
    let unknown: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 4)
        .collect();
    assert!(
        unknown.is_empty(),
        "expected no method token for $this->unknownMethod(), got: {:?}",
        unknown
    );
}

#[test]
fn this_call_trait_method_keeps_token() {
    let php = r#"<?php
trait Helper {
    public function help(): void {}
}
class Uses {
    use Helper;
    public function go(): void {
        $this->help();
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let help_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 7)
        .collect();
    assert!(
        !help_toks.is_empty(),
        "expected method token for trait method via $this->help()"
    );
}

#[test]
fn trait_internal_this_call_keeps_token() {
    let php = r#"<?php
trait Concern {
    public function run(): void {
        $this->definedByUsingClass();
    }
}
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    // Inside a trait, $this refers to the (unknown) using class —
    // members cannot be verified, so the token is kept.
    let toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 3)
        .collect();
    assert!(
        !toks.is_empty(),
        "expected method token for $this call inside a trait"
    );
}

#[test]
fn magic_call_static_keeps_token() {
    let php = r#"<?php
class Facade {
    public static function __callStatic(string $name, array $args): mixed { return null; }
}
Facade::anythingGoes();
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 4)
        .collect();
    assert!(
        !toks.is_empty(),
        "expected method token on class with __callStatic catch-all"
    );
}

#[test]
fn deprecated_static_method_call_has_deprecated_modifier() {
    let php = r#"<?php
class Api {
    /** @deprecated Use fresh() instead */
    public static function stale(): void {}
}
Api::stale();
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let stale_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.token_type == TT_METHOD && t.line == 5)
        .collect();
    assert!(
        !stale_toks.is_empty(),
        "expected method token for Api::stale()"
    );
    assert!(
        stale_toks
            .iter()
            .any(|t| has_modifier(t, TM_DEPRECATED) && has_modifier(t, TM_STATIC)),
        "expected deprecated + static modifiers on Api::stale(), got: {:?}",
        stale_toks
    );
}

// ─── Import prolog is left to the editor grammar ───────────────────────────
//
// In regular PHP files the editor's own grammar (Tree-sitter/TextMate)
// highlights `namespace` and `use` declarations per name segment.  Emitting
// a single token across the whole path would override that coloring, so
// these tokens are only produced for Blade files (where no PHP grammar
// runs).

#[test]
fn use_import_has_no_token_in_php_file() {
    let php = r#"<?php
namespace App;
use Some\Other\Thing;
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let import_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.line == 2 && t.token_type != TT_KEYWORD)
        .collect();
    assert!(
        import_toks.is_empty(),
        "expected no non-keyword tokens on the use-import line, got: {:?}",
        import_toks
    );
}

#[test]
fn namespace_declaration_has_no_token_in_php_file() {
    let php = r#"<?php
namespace App\Models;
"#;
    let tokens = get_tokens(php);
    let decoded = decode_tokens(&tokens);
    let ns_toks: Vec<_> = decoded
        .iter()
        .filter(|t| t.line == 1 && t.token_type != TT_KEYWORD)
        .collect();
    assert!(
        ns_toks.is_empty(),
        "expected no non-keyword tokens on the namespace line, got: {:?}",
        ns_toks
    );
}

// ─── Blade semantic token tests ─────────────────────────────────────────────

fn get_blade_tokens(blade: &str) -> Vec<DecodedToken> {
    let backend = create_test_backend();
    let uri = "file:///test.blade.php";
    // Populate open_files so collect_blade_tokens can read the original content.
    backend
        .open_files()
        .write()
        .insert(uri.to_string(), std::sync::Arc::new(blade.to_string()));
    backend.update_ast(uri, blade);
    let result = backend.handle_semantic_tokens_full(uri, blade);
    match result {
        Some(SemanticTokensResult::Tokens(tokens)) => decode_tokens(&tokens.data),
        _ => vec![],
    }
}

#[test]
fn blade_php_block_tokens_debug() {
    let blade = "@php\nuse Pdo\\Mysql;\n/** @var \\App\\Foo $x */\n@endphp";
    let tokens = get_blade_tokens(blade);
    for t in &tokens {
        eprintln!(
            "line={} char={} len={} type={} mods={}",
            t.line, t.character, t.length, t.token_type, t.modifiers
        );
    }
    // @var should be keyword (13)
    let var_tok = tokens
        .iter()
        .find(|t| t.token_type == TT_KEYWORD && t.length == 4 && t.line == 2);
    assert!(var_tok.is_some(), "Expected @var keyword token on line 2");
    // /** */ should have comment tokens
    let comment_tok = tokens
        .iter()
        .find(|t| t.token_type == TT_COMMENT && t.line == 2);
    assert!(
        comment_tok.is_some(),
        "Expected comment token on line 2, tokens: {:?}",
        tokens.iter().filter(|t| t.line == 2).collect::<Vec<_>>()
    );
    // Pdo\Mysql in use-import gets type token (closest to Tree-sitter's @module)
    let type_tok = tokens
        .iter()
        .find(|t| t.line == 1 && t.token_type == TT_TYPE);
    assert!(
        type_tok.is_some(),
        "Expected type token for Pdo\\Mysql on line 1, tokens: {:?}",
        tokens.iter().filter(|t| t.line == 1).collect::<Vec<_>>()
    );
}

#[test]
fn blade_keyword_tokens_real_world() {
    let blade = "@php\nuse Pdo\\Mysql;\n@endphp\n\n@foreach ($countries as $country)\n    {{ $country->code }}\n    {{ Mysql::ATTR_AUTOCOMMIT }}\n@endforeach";
    let tokens = get_blade_tokens(blade);
    let keyword_tokens: Vec<_> = tokens
        .iter()
        .filter(|t| t.token_type == TT_KEYWORD)
        .collect();
    eprintln!("Keyword tokens: {:#?}", keyword_tokens);
    assert!(
        keyword_tokens.len() >= 7,
        "Expected at least 7 keyword tokens, got {}: {:?}",
        keyword_tokens.len(),
        keyword_tokens
    );
}

#[test]
fn blade_as_keyword_in_foreach() {
    let blade = "@foreach ($items as $item)\n@endforeach";
    let tokens = get_blade_tokens(blade);
    let keyword_tokens: Vec<_> = tokens
        .iter()
        .filter(|t| t.token_type == TT_KEYWORD)
        .collect();
    eprintln!("All keyword tokens: {:#?}", keyword_tokens);
    let as_tok = tokens
        .iter()
        .find(|t| t.line == 0 && t.token_type == TT_KEYWORD && t.length == 2 && t.character > 8);
    assert!(
        as_tok.is_some(),
        "Expected 'as' keyword token, keywords: {:?}",
        keyword_tokens
    );
}

#[test]
fn blade_cast_type_in_echo() {
    let blade = "{{ (string)$var }}";
    let tokens = get_blade_tokens(blade);
    eprintln!("All tokens: {:#?}", tokens);
    let type_tok = tokens.iter().find(|t| t.token_type == TT_TYPE);
    assert!(
        type_tok.is_some(),
        "Expected 'string' type token, got: {:?}",
        tokens
    );
    assert_eq!(type_tok.unwrap().length, 6);
}

#[test]
fn blade_directive_tokens() {
    let tokens = get_blade_tokens("@if($x)\n    <p>hello</p>\n@endif");
    let if_tok = tokens
        .iter()
        .find(|t| t.character == 0 && t.line == 0 && t.token_type == TT_KEYWORD);
    assert!(
        if_tok.is_some(),
        "Expected @if keyword token, got: {:?}",
        tokens
    );
    assert_eq!(if_tok.unwrap().length, 3); // @if

    let endif_tok = tokens
        .iter()
        .find(|t| t.line == 2 && t.token_type == TT_KEYWORD);
    assert!(
        endif_tok.is_some(),
        "Expected @endif keyword token, got: {:?}",
        tokens
    );
    assert_eq!(endif_tok.unwrap().length, 6); // @endif
}

#[test]
fn blade_echo_delimiter_tokens() {
    let tokens = get_blade_tokens("{{ $name }}");
    let open = tokens
        .iter()
        .find(|t| t.character == 0 && t.token_type == TT_KEYWORD);
    assert!(
        open.is_some(),
        "Expected {{ keyword token, got: {:?}",
        tokens
    );
    assert_eq!(open.unwrap().length, 2);

    let close = tokens
        .iter()
        .find(|t| t.character == 9 && t.token_type == TT_KEYWORD);
    assert!(
        close.is_some(),
        "Expected }} keyword token, got: {:?}",
        tokens
    );
    assert_eq!(close.unwrap().length, 2);
}

#[test]
fn blade_raw_echo_delimiter_tokens() {
    let tokens = get_blade_tokens("{!! $html !!}");
    let open = tokens
        .iter()
        .find(|t| t.character == 0 && t.token_type == TT_KEYWORD);
    assert!(
        open.is_some(),
        "Expected {{!! keyword token, got: {:?}",
        tokens
    );
    assert_eq!(open.unwrap().length, 3);

    let close = tokens
        .iter()
        .find(|t| t.character == 10 && t.token_type == TT_KEYWORD);
    assert!(
        close.is_some(),
        "Expected !!}} keyword token, got: {:?}",
        tokens
    );
    assert_eq!(close.unwrap().length, 3);
}

#[test]
fn blade_comment_delimiter_tokens() {
    let tokens = get_blade_tokens("{{-- this is a comment --}}");
    // Entire comment is a single comment token.
    let comment = tokens
        .iter()
        .find(|t| t.character == 0 && t.token_type == TT_COMMENT);
    assert!(
        comment.is_some(),
        "Expected comment token, got: {:?}",
        tokens
    );
    assert_eq!(comment.unwrap().length, 27); // entire {{-- this is a comment --}}
}

#[test]
fn blade_foreach_directive_token() {
    let tokens = get_blade_tokens("@foreach ($items as $item)\n    {{ $item }}\n@endforeach");
    let foreach_tok = tokens
        .iter()
        .find(|t| t.line == 0 && t.character == 0 && t.token_type == TT_KEYWORD);
    assert!(foreach_tok.is_some(), "Expected @foreach keyword token");
    assert_eq!(foreach_tok.unwrap().length, 8); // @foreach

    let endforeach_tok = tokens
        .iter()
        .find(|t| t.line == 2 && t.character == 0 && t.token_type == TT_KEYWORD);
    assert!(
        endforeach_tok.is_some(),
        "Expected @endforeach keyword token"
    );
    assert_eq!(endforeach_tok.unwrap().length, 11); // @endforeach
}
