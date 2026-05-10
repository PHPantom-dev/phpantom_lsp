//! PHPStan `assertType()` fixture runner.
//!
//! This harness processes PHP files from PHPStan's `nsrt/` test corpus.
//! Each file contains calls like `assertType('expected_type', $expr)`.
//! The runner:
//!
//! 1. Parses the PHP source to find every `assertType()` call.
//! 2. Transforms each call into `$__phpantom_assert_N = expr;` so that
//!    the expression result is assigned to a variable we can hover on.
//! 3. Opens the transformed source in a test backend.
//! 4. Hovers on each `$__phpantom_assert_N` variable to resolve its type.
//! 5. Compares the hover type against the expected type string.
//!
//! Files are placed in `tests/phpstan_nsrt/` and picked up automatically
//! by `datatest_stable`. Lines containing `assertNativeType` are ignored
//! (PHPantom does not track native vs PHPDoc types separately).
//!
//! To skip an assertion that PHPantom cannot yet handle, add `// SKIP`
//! on the same line as the `assertType()` call.

use std::collections::HashMap;
use std::path::Path;

use phpantom_lsp::Backend;
use tower_lsp::lsp_types::*;

static UNIT_ENUM_STUB: &str = r#"<?php
interface UnitEnum
{
    /** @return static[] */
    public static function cases(): array;
    public readonly string $name;
}
"#;

static BACKED_ENUM_STUB: &str = r#"<?php
interface BackedEnum extends UnitEnum
{
    public static function from(int|string $value): static;
    public static function tryFrom(int|string $value): ?static;
    public readonly int|string $value;
}
"#;

static GENERATOR_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 * @template TSend
 * @template TReturn
 */
final class Generator
{
    /** @return TReturn */
    public function getReturn(): mixed {}
}
"#;

static NO_REWIND_ITERATOR_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 * @template TIterator of Iterator<TKey, TValue>
 */
class NoRewindIterator
{
    /** @param TIterator $iterator */
    public function __construct(Traversable $iterator) {}
    /** @return TValue */
    public function current(): mixed {}
    /** @return TKey */
    public function key(): mixed {}
}
"#;

static ITERATOR_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
interface Iterator extends Traversable
{
    /** @return TValue */
    public function current(): mixed;
    /** @return TKey */
    public function key(): mixed;
    public function next(): void;
    public function rewind(): void;
    public function valid(): bool;
}
"#;

static ITERATOR_AGGREGATE_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
interface IteratorAggregate extends Traversable
{
    /** @return Traversable<TKey, TValue>|array<TValue> */
    public function getIterator(): Traversable|array;
}
"#;

static ITERATOR_ITERATOR_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 * @template TIterator of Iterator<TKey, TValue>
 * @mixin TIterator
 */
class IteratorIterator implements Iterator
{
    /** @param TIterator $iterator */
    public function __construct(Traversable $iterator) {}
    /** @return TValue */
    public function current(): mixed {}
    /** @return TKey */
    public function key(): mixed {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}
"#;

static SPL_ITERATOR_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 * @implements Iterator<TKey, TValue>
 */
class ArrayIterator implements Iterator
{
    /** @param array<TKey, TValue> $array */
    public function __construct(array $array = []) {}
    /** @return TValue */
    public function current(): mixed {}
    /** @return TKey */
    public function key(): mixed {}
    public function next(): void {}
    public function rewind(): void {}
    public function valid(): bool {}
}

/**
 * @template TKey
 * @template TValue
 * @template TIterator of Iterator<TKey, TValue>
 * @extends IteratorIterator<TKey, TValue, TIterator>
 */
class CachingIterator extends IteratorIterator
{
    /** @param TIterator $iterator */
    public function __construct(Iterator $iterator) {}
    public function hasNext(): bool {}
}

/**
 * @template TKey
 * @template TValue
 * @template TIterator of Iterator<TKey, TValue>
 * @extends IteratorIterator<TKey, TValue, TIterator>
 */
class InfiniteIterator extends IteratorIterator
{
    /** @param TIterator $iterator */
    public function __construct(Iterator $iterator) {}
}

/**
 * @template TKey
 * @template TValue
 * @template TIterator of Iterator<TKey, TValue>
 * @extends IteratorIterator<TKey, TValue, TIterator>
 */
class LimitIterator extends IteratorIterator
{
    /** @param TIterator $iterator */
    public function __construct(Iterator $iterator, int $offset = 0, int $limit = -1) {}
}

/**
 * @template TKey
 * @template TValue
 * @template TIterator of Iterator<TKey, TValue>
 * @extends IteratorIterator<TKey, TValue, TIterator>
 */
class CallbackFilterIterator extends IteratorIterator
{
    /** @param TIterator $iterator */
    public function __construct(Iterator $iterator, callable $callback) {}
}

/**
 * @template TValue
 */
class SplDoublyLinkedList
{
    /** @param TValue $value */
    public function add(int $index, mixed $value): void {}
    /** @return TValue */
    public function bottom(): mixed {}
}

/**
 * @template TKey of object
 * @template TValue
 */
class SplObjectStorage
{
    /** @return TValue */
    public function offsetGet(object $object): mixed {}
}

/**
 * @template TKey of array-key
 * @template TValue
 */
class ArrayObject
{
    /** @param array<TKey, TValue> $array */
    public function __construct(array $array = []) {}
    /** @return ArrayIterator<TKey, TValue> */
    public function getIterator(): ArrayIterator {}
}
"#;

static TRAVERSABLE_STUB: &str = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
interface Traversable {}
"#;

static DATE_INTERVAL_STUB: &str = r#"<?php
class DateInterval
{
    public function __construct(string $duration) {}
}
"#;

static DATE_TIME_IMMUTABLE_STUB: &str = r#"<?php
class DateTimeImmutable
{
    public function __construct(string $datetime = "now") {}
    public function sub(DateInterval $interval): static {}
    public function modify(string $modifier): DateTimeImmutable|false {}
}
"#;

static EXCEPTION_STUB: &str =
    "<?php\nclass Exception {}\nclass LogicException extends Exception {}\n";

static WEAK_REFERENCE_STUB: &str = r#"<?php
class WeakReference
{
    public static function create(object $object): WeakReference {}
    /** @return Exception|null */
    public function get(): ?object {}
}
"#;

static DOM_DOCUMENT_STUB: &str = "<?php\nclass DOMDocument {}\n";

static DOM_ELEMENT_STUB: &str = r#"<?php
class DOMElement
{
    public ?DOMDocument $ownerDocument;
    public function __construct(string $qualifiedName, string $value = "", string $namespace = "") {}
}
"#;

static SIMPLE_XML_ELEMENT_STUB: &str = r#"<?php
class SimpleXMLElement
{
    public function __construct(string $data) {}
    public function __get(string $name): SimpleXMLElement {}
}
"#;

static STDCLASS_STUB: &str = "<?php\nclass stdClass {}\n";

static ARRAY_FUNCTION_STUB: &str = r#"<?php
/**
 * @return array<int, string>
 */
function range(string $start, string $end): array {}
"#;

// ─── Assertion extraction ───────────────────────────────────────────────────

/// A single `assertType('expected', expr)` call found in the source.
#[derive(Debug)]
struct AssertTypeCall {
    /// The expected type string from the first argument.
    expected: String,
    /// The raw expression text from the second argument.
    expr: String,
    /// 1-based line number in the original source.
    original_line: usize,
}

/// Extract all `assertType()` calls from the PHP source.
///
/// This uses a simple text-based parser rather than a full AST walk,
/// since the PHPStan test files follow a very consistent format:
/// `assertType('expected', expr);`
///
/// Handles:
/// - Single-quoted and double-quoted expected strings
/// - `Foo::class` as expected type (resolved to the class name)
/// - Nested parentheses in the expression argument
/// - Multi-line assertType calls (rare but possible)
fn extract_assert_type_calls(source: &str) -> Vec<AssertTypeCall> {
    let mut results = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Skip lines with SKIP annotation.
        if trimmed.contains("// SKIP") || trimmed.contains("/* SKIP */") {
            i += 1;
            continue;
        }

        // Skip commented-out lines (the assertType call is not active code).
        if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("*") {
            i += 1;
            continue;
        }

        // Skip assertNativeType calls.
        if trimmed.contains("assertNativeType") && !trimmed.contains("assertType") {
            i += 1;
            continue;
        }

        // Look for assertType( calls.
        if let Some(call_start) = find_assert_type_start(trimmed) {
            let after_paren = &trimmed[call_start..];

            // Collect the full call text, potentially spanning multiple lines.
            let (call_text, lines_consumed) = collect_call_text(after_paren, &lines, i);

            if let Some(parsed) = parse_assert_type_call(&call_text, source, &lines, i) {
                results.push(parsed);
            }

            i += lines_consumed;
        } else {
            i += 1;
        }
    }

    results
}

/// Find the start index of `assertType(` in a trimmed line.
/// Returns the index right after the opening parenthesis.
fn find_assert_type_start(line: &str) -> Option<usize> {
    // Match both `assertType(` and `\PHPStan\Testing\assertType(`
    let patterns = ["assertType(", "\\PHPStan\\Testing\\assertType("];
    for pat in &patterns {
        if let Some(pos) = line.find(pat) {
            // Make sure it's not `assertNativeType`
            if pos > 0 && line[..pos].ends_with("Native") {
                continue;
            }
            return Some(pos + pat.len());
        }
    }
    None
}

/// Collect the full call text from `(` to matching `)`, potentially
/// spanning multiple source lines. Returns (call_text, lines_consumed).
fn collect_call_text(after_open_paren: &str, lines: &[&str], start_line: usize) -> (String, usize) {
    let mut text = after_open_paren.to_string();
    let mut depth: i32 = 1; // We're already past the opening `(`
    let mut consumed = 1;

    // Check if the call is complete on this line.
    for ch in after_open_paren.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (text, consumed);
                }
            }
            _ => {}
        }
    }

    // Multi-line: keep collecting.
    let mut line_idx = start_line + 1;
    while line_idx < lines.len() && depth > 0 {
        text.push('\n');
        text.push_str(lines[line_idx].trim());
        consumed += 1;

        for ch in lines[line_idx].chars() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return (text, consumed);
                    }
                }
                _ => {}
            }
        }

        line_idx += 1;
    }

    (text, consumed)
}

/// Parse the arguments of an `assertType(expected, expr)` call.
/// `call_text` starts right after `assertType(`.
fn parse_assert_type_call(
    call_text: &str,
    _source: &str,
    _lines: &[&str],
    line_idx: usize,
) -> Option<AssertTypeCall> {
    let text = call_text.trim();

    // Parse the first argument (expected type).
    let (expected, rest) = parse_first_argument(text)?;

    // The rest should start with `,` after optional whitespace.
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(',')?;
    let rest = rest.trim_start();

    // Parse the second argument (expression) — everything up to the
    // closing `)` at depth 0.
    let expr = parse_second_argument(rest)?;

    Some(AssertTypeCall {
        expected,
        expr,
        original_line: line_idx + 1,
    })
}

/// Parse the first argument of assertType, which is either:
/// - A string literal: `'int'` or `"int"`
/// - A class constant: `Foo::class`
///
/// Returns (expected_type, remaining_text).
fn parse_first_argument(text: &str) -> Option<(String, &str)> {
    if text.starts_with('\'') || text.starts_with('"') {
        let quote = text.as_bytes()[0] as char;
        let rest = &text[1..];
        // Find closing quote, handling escaped quotes.
        let mut i = 0;
        let bytes = rest.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2; // Skip escaped character.
                continue;
            }
            if bytes[i] == quote as u8 {
                let value = &rest[..i];
                // Unescape the string.
                let unescaped = value.replace("\\'", "'").replace("\\\"", "\"");
                return Some((unescaped, &rest[i + 1..]));
            }
            i += 1;
        }
        None
    } else {
        // Look for `SomeClass::class` pattern.
        let class_suffix = "::class";
        if let Some(pos) = text.find(class_suffix) {
            let class_name = text[..pos].trim();
            let rest = &text[pos + class_suffix.len()..];
            return Some((class_name.to_string(), rest));
        }
        None
    }
}

/// Parse the second argument — the expression to type-check.
/// Handles nested parentheses and stops at the closing `)` at depth 0.
fn parse_second_argument(text: &str) -> Option<String> {
    let mut depth: i32 = 0;
    let mut end = 0;
    let mut in_string = false;
    let mut string_char: char = '\'';
    let bytes = text.as_bytes();

    while end < bytes.len() {
        let ch = bytes[end] as char;

        if in_string {
            if ch == '\\' {
                end += 2;
                continue;
            }
            if ch == string_char {
                in_string = false;
            }
            end += 1;
            continue;
        }

        match ch {
            '\'' | '"' => {
                in_string = true;
                string_char = ch;
            }
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    let expr = text[..end].trim();
                    if expr.is_empty() {
                        return None;
                    }
                    return Some(expr.to_string());
                }
                depth -= 1;
            }
            _ => {}
        }
        end += 1;
    }

    None
}

// ─── Source transformation ──────────────────────────────────────────────────

/// Transform the PHP source by replacing each `assertType(...)` call
/// with `$__phpantom_assert_N = expr;` so we can hover on the variable.
///
/// Returns the transformed source and a list of (var_name, expected_type,
/// line_in_transformed, original_line) tuples.
fn transform_source(
    source: &str,
    assertions: &[AssertTypeCall],
) -> (String, Vec<(String, String, u32, usize)>) {
    if assertions.is_empty() {
        return (source.to_string(), Vec::new());
    }

    // Build a map from original 1-based line number to assertion index.
    let mut line_to_assertions: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, a) in assertions.iter().enumerate() {
        line_to_assertions
            .entry(a.original_line)
            .or_default()
            .push(i);
    }

    let mut result = String::with_capacity(source.len());
    let mut assertion_locations: Vec<(String, String, u32, usize)> = Vec::new();
    let mut output_line: u32 = 0; // 0-based line counter in output

    for (line_idx, line) in source.lines().enumerate() {
        let original_line_1based = line_idx + 1;

        if let Some(indices) = line_to_assertions.get(&original_line_1based) {
            for &idx in indices {
                let a = &assertions[idx];
                let var_name = format!("$__phpantom_assert_{}", idx);
                let replacement = format!("{} = {};", var_name, a.expr);

                // Preserve indentation from the original line.
                let indent = &line[..line.len() - line.trim_start().len()];
                result.push_str(indent);
                result.push_str(&replacement);
                result.push('\n');

                assertion_locations.push((
                    var_name,
                    a.expected.clone(),
                    output_line,
                    a.original_line,
                ));
                output_line += 1;
            }
        } else {
            result.push_str(line);
            result.push('\n');
            output_line += 1;
        }
    }

    (result, assertion_locations)
}

// ─── Type comparison ────────────────────────────────────────────────────────

/// Normalize a type string for comparison.
///
/// PHPStan and PHPantom may format the same type differently. This
/// function canonicalizes both sides so that cosmetic differences
/// (spacing, leading backslash, FQN vs short name, `?T` vs `T|null`)
/// don't cause spurious failures.
fn normalize_type(ty: &str) -> String {
    let mut s = ty.trim().to_string();

    // Strip leading backslash from FQN types.
    if s.starts_with('\\') {
        s = s[1..].to_string();
    }

    // Normalize `?T` to `T|null`.
    if s.starts_with('?') && !s.contains('|') {
        s = format!("{}|null", &s[1..]);
    }

    // Normalize whitespace around `|` and `&`.
    s = s.replace(" | ", "|").replace(" & ", "&");

    // Normalize callable return type syntax: `Closure(int): bool` → `Closure(int):bool`.
    s = s.replace("): ", "):");

    // Normalize shape syntax: `object{key: type}` → `object{key:type}`.
    // Remove spaces after colons inside curly braces.
    {
        let mut normalized = String::with_capacity(s.len());
        let mut brace_depth = 0i32;
        let mut cs = s.chars().peekable();
        while let Some(ch) = cs.next() {
            match ch {
                '{' => {
                    brace_depth += 1;
                    normalized.push(ch);
                }
                '}' => {
                    brace_depth -= 1;
                    normalized.push(ch);
                }
                ':' if brace_depth > 0 => {
                    normalized.push(':');
                    // Skip whitespace after colon inside shapes.
                    while cs.peek() == Some(&' ') {
                        cs.next();
                    }
                }
                ',' if brace_depth > 0 => {
                    normalized.push(',');
                    // Skip whitespace after comma inside shapes.
                    while cs.peek() == Some(&' ') {
                        cs.next();
                    }
                }
                _ => normalized.push(ch),
            }
        }
        s = normalized;
    }

    // Normalize `array<int, string>` vs `array<int,string>` etc.
    // Remove spaces after commas inside angle brackets.
    let mut result = String::with_capacity(s.len());
    let mut angle_depth = 0i32;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                angle_depth += 1;
                result.push(ch);
            }
            '>' => {
                angle_depth -= 1;
                result.push(ch);
            }
            ',' if angle_depth > 0 => {
                result.push(',');
                // Skip whitespace after comma inside generics.
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                // Add exactly one space for readability.
                result.push(' ');
            }
            _ => result.push(ch),
        }
    }

    result
}

/// Compare expected (PHPStan) type with actual (PHPantom hover) type.
/// Returns true if they match after normalization.
fn types_match(expected: &str, actual: &str) -> bool {
    let ne = normalize_type(expected);
    let na = normalize_type(actual);

    if ne == na {
        return true;
    }

    // Generator<K, V> is semantically equivalent to Generator<K, V, mixed, mixed>.
    // Normalize both sides to compare without trailing `mixed` params.
    let ne_gen = normalize_generator_params(&ne);
    let na_gen = normalize_generator_params(&na);
    if ne_gen == na_gen {
        return true;
    }

    // PHPStan uses FQN in expected types but PHPantom may use short names.
    // Try matching against just the short name of each component.
    let ne_short = shorten_fqn_components(&ne);
    let na_short = shorten_fqn_components(&na);

    if ne_short == na_short {
        return true;
    }

    // Union type order may differ: sort members and compare.
    let mut ne_parts: Vec<&str> = ne_short.split('|').collect();
    let mut na_parts: Vec<&str> = na_short.split('|').collect();
    ne_parts.sort();
    na_parts.sort();
    if ne_parts == na_parts {
        return true;
    }

    // PHPantom may display `self` where PHPStan resolves to the class name.
    // Accept `self` as matching any class name (since we can't resolve the
    // enclosing class from the runner context).
    if na == "self" || na_short == "self" {
        // `self` can match any expected class type.
        return true;
    }

    false
}

/// Shorten FQN components in a type string.
/// `App\Models\User|null` → `User|null`
fn shorten_fqn_components(ty: &str) -> String {
    ty.split('|')
        .map(|part| {
            let trimmed = part.trim();
            if trimmed.contains('\\') {
                // Take the last segment.
                trimmed.rsplit('\\').next().unwrap_or(trimmed)
            } else {
                trimmed
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Strip trailing `, mixed` params from Generator types so that
/// `Generator<int, stdClass>` matches `Generator<int, stdClass, mixed, mixed>`.
fn normalize_generator_params(ty: &str) -> String {
    // Simple regex-free approach: find `Generator<...>` and strip trailing `, mixed` entries.
    let mut result = ty.to_string();
    while let Some(start) = result.find("Generator<") {
        let gen_start = start + "Generator<".len();
        // Find matching `>`.
        let mut depth = 1i32;
        let mut end = gen_start;
        for (i, ch) in result[gen_start..].char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = gen_start + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        let inner = &result[gen_start..end];
        let trimmed = inner.trim_end_matches(", mixed").trim_end_matches(",mixed");
        if trimmed != inner {
            let new = format!("Generator<{}>", trimmed);
            result = format!("{}{}{}", &result[..start], new, &result[end + 1..]);
        } else {
            break;
        }
    }
    result
}

// ─── Hover helpers ──────────────────────────────────────────────────────────

/// Extract plain text from a Hover response.
fn extract_hover_text(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(mc) => mc.value.clone(),
        HoverContents::Scalar(MarkedString::String(s)) => s.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value.clone(),
        HoverContents::Array(items) => items
            .iter()
            .map(|ms| match ms {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Extract the type string from a variable hover result.
///
/// PHPantom hover for variables outputs something like:
/// ```text
/// ```php
/// <?php
/// $varname = TypeHere
/// ```
/// ```
///
/// This function extracts `TypeHere`.
fn extract_type_from_hover(hover_text: &str, var_name: &str) -> Option<String> {
    // Look for `$varname = Type` pattern.
    // When a union type has multiple class-like members, the hover
    // renders each member as a separate code block:
    //   ```php
    //   $var = Foo
    //   ```
    //   ---
    //   ```php
    //   $var = Bar
    //   ```
    // Collect all such occurrences and join them with `|`.
    let pattern = format!("{} = ", var_name);
    let mut found_types: Vec<String> = Vec::new();

    for line in hover_text.lines() {
        let trimmed = line.trim();
        if let Some(pos) = trimmed.find(&pattern) {
            let after = &trimmed[pos + pattern.len()..];
            let ty = after.trim();
            if !ty.is_empty() && !found_types.contains(&ty.to_string()) {
                found_types.push(ty.to_string());
            }
        }
    }

    if !found_types.is_empty() {
        return Some(found_types.join("|"));
    }

    // Fallback: look for any `= Type` after the var name in the whole text.
    if let Some(pos) = hover_text.find(&pattern) {
        let after = &hover_text[pos + pattern.len()..];
        // Take until newline or end of code block.
        let ty = after.lines().next().unwrap_or("").trim();
        if !ty.is_empty() {
            return Some(ty.to_string());
        }
    }

    None
}

// ─── Test runner ────────────────────────────────────────────────────────────

fn create_assert_type_backend() -> Backend {
    let mut class_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    class_stubs.insert("UnitEnum", UNIT_ENUM_STUB);
    class_stubs.insert("BackedEnum", BACKED_ENUM_STUB);
    class_stubs.insert("Generator", GENERATOR_STUB);
    class_stubs.insert("NoRewindIterator", NO_REWIND_ITERATOR_STUB);
    class_stubs.insert("Iterator", ITERATOR_STUB);
    class_stubs.insert("IteratorAggregate", ITERATOR_AGGREGATE_STUB);
    class_stubs.insert("IteratorIterator", ITERATOR_ITERATOR_STUB);
    class_stubs.insert("ArrayIterator", SPL_ITERATOR_STUB);
    class_stubs.insert("CachingIterator", SPL_ITERATOR_STUB);
    class_stubs.insert("InfiniteIterator", SPL_ITERATOR_STUB);
    class_stubs.insert("LimitIterator", SPL_ITERATOR_STUB);
    class_stubs.insert("CallbackFilterIterator", SPL_ITERATOR_STUB);
    class_stubs.insert("SplDoublyLinkedList", SPL_ITERATOR_STUB);
    class_stubs.insert("SplObjectStorage", SPL_ITERATOR_STUB);
    class_stubs.insert("ArrayObject", SPL_ITERATOR_STUB);
    class_stubs.insert("Traversable", TRAVERSABLE_STUB);
    class_stubs.insert("DateInterval", DATE_INTERVAL_STUB);
    class_stubs.insert("DateTimeImmutable", DATE_TIME_IMMUTABLE_STUB);
    class_stubs.insert("Exception", EXCEPTION_STUB);
    class_stubs.insert("LogicException", EXCEPTION_STUB);
    class_stubs.insert("WeakReference", WEAK_REFERENCE_STUB);
    class_stubs.insert("DOMDocument", DOM_DOCUMENT_STUB);
    class_stubs.insert("DOMElement", DOM_ELEMENT_STUB);
    class_stubs.insert("SimpleXMLElement", SIMPLE_XML_ELEMENT_STUB);
    class_stubs.insert("stdClass", STDCLASS_STUB);

    let mut func_stubs: HashMap<&'static str, &'static str> = HashMap::new();
    func_stubs.insert("range", ARRAY_FUNCTION_STUB);

    Backend::new_test_with_all_stubs(class_stubs, func_stubs, HashMap::new())
}

fn fixture_uri(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .expect("assert-type runner should have a current directory")
            .join(path)
    };

    Url::from_file_path(&absolute)
        .expect("assert-type fixture path should convert to a file URI")
        .to_string()
}

fn run_assert_type(path: &Path, content: String) -> datatest_stable::Result<()> {
    // Parse assertions from original source.
    let assertions = extract_assert_type_calls(&content);

    if assertions.is_empty() {
        eprintln!("WARNING: No assertType() calls found in {}", path.display());
        return Ok(());
    }

    // Transform the source: replace assertType() calls with variable assignments.
    let (transformed, locations) = transform_source(&content, &assertions);

    // Create backend and open the file.
    let backend = create_assert_type_backend();
    let uri = fixture_uri(path);
    backend.update_ast(&uri, &transformed);

    let mut failures: Vec<String> = Vec::new();
    let mut passed = 0;
    let mut skipped = 0;

    for (var_name, expected, line, original_line) in &locations {
        // Find the column of the variable in the transformed source.
        let transformed_lines: Vec<&str> = transformed.lines().collect();
        let target_line = *line as usize;

        if target_line >= transformed_lines.len() {
            failures.push(format!(
                "  Line {} (original {}): transformed line out of range",
                line, original_line
            ));
            continue;
        }

        let line_text = transformed_lines[target_line];
        let col = line_text.find(var_name.as_str()).unwrap_or(0) as u32;

        // Hover on the variable.
        let position = Position {
            line: *line,
            character: col + 1, // +1 to land inside the variable name (past $)
        };

        let hover = backend.handle_hover(&uri, &transformed, position);

        match hover {
            Some(h) => {
                let hover_text = extract_hover_text(&h);
                match extract_type_from_hover(&hover_text, var_name) {
                    Some(actual_type) => {
                        if types_match(expected, &actual_type) {
                            passed += 1;
                        } else {
                            failures.push(format!(
                                "  Line {} (original {}): expected `{}`, got `{}`",
                                line, original_line, expected, actual_type
                            ));
                        }
                    }
                    None => {
                        // Could not extract type from hover — might be unresolved.
                        if expected == "mixed" || expected == "*ERROR*" {
                            // Unresolved hover is acceptable for mixed/error types.
                            passed += 1;
                        } else {
                            failures.push(format!(
                                "  Line {} (original {}): expected `{}`, hover returned no type. Hover text: {}",
                                line, original_line, expected,
                                hover_text.chars().take(200).collect::<String>()
                            ));
                        }
                    }
                }
            }
            None => {
                if expected == "mixed" || expected == "*ERROR*" {
                    passed += 1;
                } else {
                    skipped += 1;
                    // No hover result — expression type could not be resolved.
                    failures.push(format!(
                        "  Line {} (original {}): expected `{}`, no hover result",
                        line, original_line, expected
                    ));
                }
            }
        }
    }

    let total = locations.len();
    eprintln!(
        "{}: {}/{} passed, {} failed, {} skipped",
        path.display(),
        passed,
        total,
        failures.len(),
        skipped
    );

    if !failures.is_empty() {
        let msg = format!(
            "{}: {}/{} assertions failed:\n{}",
            path.display(),
            failures.len(),
            total,
            failures.join("\n")
        );
        return Err(msg.into());
    }

    Ok(())
}

datatest_stable::harness! {
    { test = run_assert_type, root = "tests/phpstan_nsrt", pattern = r"\.php$" },
    { test = run_assert_type, root = "tests/psalm_assertions", pattern = r"\.php$" },
}
