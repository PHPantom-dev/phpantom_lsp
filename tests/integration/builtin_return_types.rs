//! Integration tests for builtins whose return type is decided by an
//! argument.
//!
//! The stubs can only declare the union of every shape a function can return,
//! so a call that provably takes one branch still carries the others. Each
//! test here pins one such function to the branch its arguments select, and
//! checks that a call whose argument cannot be pinned down keeps both.

use crate::common::{assert_assigned_types, create_test_backend_with_full_stubs, hover_at};
use tower_lsp::lsp_types::*;

/// Assert the type of each assignment in `content`, keyed by the variable it
/// assigns to. Line numbers are found by scanning for `$name = `.
/// `pathinfo()` only returns the component array for the all-elements form:
/// every other flag asks for one part and gets a string. `PATHINFO_ALL` is
/// the parameter's default, so the one-argument call takes the array branch.
#[test]
fn pathinfo_returns_a_string_for_a_single_component() {
    const SHAPE: &str =
        "array{dirname: string, basename: string, extension?: string, filename: string}";
    let content = r#"<?php
function probe(string $path, int $flags): void {
    $all = pathinfo($path);
    $allExplicit = pathinfo($path, PATHINFO_ALL);
    $filename = pathinfo($path, PATHINFO_FILENAME);
    $extension = pathinfo($path, \PATHINFO_EXTENSION);
    $unknown = pathinfo($path, $flags);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$all", SHAPE),
            ("$allExplicit", SHAPE),
            ("$filename", "string"),
            ("$extension", "string"),
            ("$unknown", &format!("{SHAPE}|string")),
        ],
    );
}

/// `print_r()` returns the rendered string only when asked to; otherwise it
/// prints and reports that it did. The declared `string|bool` carries a
/// `false` php-src never returns.
#[test]
fn print_r_returns_a_string_only_when_asked_to() {
    let content = r#"<?php
function probe(mixed $value, bool $capture): void {
    $printed = print_r($value);
    $rendered = print_r($value, true);
    $notRendered = print_r($value, false);
    $unknown = print_r($value, $capture);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$printed", "true"),
            ("$rendered", "string"),
            ("$notRendered", "true"),
            ("$unknown", "string|true"),
        ],
    );
}

/// `hrtime(true)` is a number; the `[seconds, nanoseconds]` pair is what the
/// default form returns.
#[test]
fn hrtime_follows_its_as_number_argument() {
    let content = r#"<?php
function probe(bool $asNumber): void {
    $number = hrtime(true);
    $pair = hrtime();
    $pairExplicit = hrtime(false);
    $unknown = hrtime($asNumber);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$number", "int|float"),
            ("$pair", "array{int, int}|false"),
            ("$pairExplicit", "array{int, int}|false"),
            ("$unknown", "int|float|array{int, int}|false"),
        ],
    );
}

/// `microtime()`'s stub carries `#[TypeContract(true: 'float', false:
/// 'string')]`, which says exactly which branch each call takes.
#[test]
fn microtime_follows_its_as_float_argument() {
    let content = r#"<?php
function probe(bool $asFloat): void {
    $seconds = microtime(true);
    $text = microtime();
    $unknown = microtime($asFloat);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$seconds", "float"),
            ("$text", "string"),
            ("$unknown", "float|string"),
        ],
    );
}

/// Only the no-argument `getenv()` returns the whole environment.
#[test]
fn getenv_returns_the_environment_only_without_a_name() {
    let content = r#"<?php
function probe(string $name): void {
    $one = getenv('HOME');
    $all = getenv();
    $named = getenv($name);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$one", "string|false"),
            ("$all", "array<string, string>"),
            ("$named", "string|false"),
        ],
    );
}

/// `mb_convert_encoding()` answers in the shape it was handed, like the
/// replace family. An array subject is converted per element, so no error
/// branch survives there.
#[test]
fn mb_convert_encoding_follows_its_subject() {
    let content = r#"<?php
/**
 * @param list<string> $lines
 */
function probe(string $text, array $lines, mixed $anything): void {
    $one = mb_convert_encoding($text, 'UTF-8');
    $many = mb_convert_encoding($lines, 'UTF-8');
    $unknown = mb_convert_encoding($anything, 'UTF-8');
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$one", "string|false"),
            ("$many", "array<array-key, string>"),
            ("$unknown", "array<array-key, string>|string|false"),
        ],
    );
}

/// `abs()` returns the type it was given; the declared `int|float` leaves an
/// `int` argument's result carrying a `float` branch that cannot happen.
#[test]
fn abs_returns_the_type_it_was_given() {
    let content = r#"<?php
function probe(int $i, float $f, mixed $anything): void {
    $whole = abs($i);
    $fractional = abs($f);
    $unknown = abs($anything);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$whole", "int"),
            ("$fractional", "float"),
            ("$unknown", "int|float"),
        ],
    );
}

/// `SimpleXMLElement::asXML()` / `saveXML()` serialise to a string without a
/// filename and report success with one. The declared `string|bool` splits
/// neither result.
#[test]
fn simple_xml_element_serialisers_follow_their_filename() {
    let content = r#"<?php
function probe(\SimpleXMLElement $xml, string $path): void {
    $serialised = $xml->asXML();
    $written = $xml->saveXML('/tmp/out.xml');
    $writtenToPath = $xml->asXML($path);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$serialised", "string|false"),
            ("$written", "bool"),
            ("$writtenToPath", "bool"),
        ],
    );
}

/// `str_replace()`'s conditional return is decided by the subject's type,
/// which means the subject expression has to resolve to one. A `??` argument
/// came back untyped and left the call carrying both branches.
#[test]
fn a_coalesce_argument_decides_a_conditional_return() {
    let content = r#"<?php
/**
 * @param array<string, string> $rows
 */
function probe(?string $error, ?array $rows, string $fallback): void {
    $coalesced = str_replace('Error: ', '', $error ?? '');
    $chained = str_replace('Error: ', '', $error ?? $fallback);
    $arrayArm = str_replace('Error: ', '', $rows ?? []);
    $eitherArm = str_replace('Error: ', '', $error ?? $rows);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$coalesced", "string"),
            ("$chained", "string"),
            ("$arrayArm", "array<array-key, string>"),
            ("$eitherArm", "array<array-key, string>|string"),
        ],
    );
}

/// `max()`/`min()` hand back one of the values they were given: an element of
/// a single iterable argument, or one of the arguments themselves. The stubs
/// can only say `mixed`, which lets any result through unchecked.
#[test]
fn min_and_max_answer_with_the_values_they_compare() {
    let content = r#"<?php
/**
 * @param list<string> $names
 * @param array<int, float> $weights
 */
function probe(array $names, array $weights, int $count, mixed $anything): void {
    $letter = max("a", "b");
    $widest = max($names);
    $mixedNumbers = min(1, 2.5);
    $lightest = min($weights);
    $bounded = max($count, 0);
    $unknown = max($anything, $anything);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$letter", "string"),
            ("$widest", "string"),
            ("$mixedNumbers", "1"),
            ("$lightest", "float"),
            ("$bounded", "int"),
            ("$unknown", "mixed"),
        ],
    );
}

/// Two more `max()`/`min()` pitfalls beyond picking the right branch:
///
/// 1. `max($t, filemtime('a'))` must not double the `int` argument into the
///    result nor degrade `filemtime()`'s `false` arm to `bool` — a
///    `?:`/`!==` check downstream depends on the exact `false`.
/// 2. A `max()` call whose argument is an arithmetic expression must still
///    resolve to `int` so a write key built from it (`$result[$line]`)
///    keeps `int` rather than widening to `int|string`, in both a
///    top-level function and a class method.
#[test]
fn min_and_max_preserve_false_and_do_not_poison_array_keys() {
    let content = r#"<?php
function track(int $t): void {
    $m = max($t, filemtime('a'));
    $safe = max($t, filemtime('a')) ?: 0;
}

function linesFn(int $a, int $b = 0): array {
    $line = max($a - 1 - $b, 0);
    $result = [];
    $result[$line] = 'x';
    return $result;
}

final class Excerpt {
    public function lines(int $a, int $b = 0): array {
        $line = max($a - 1 - $b, 0);
        $result = [];
        $result[$line] = 'x';
        return $result;
    }
}
"#;
    assert_assigned_types(content, &[("$m", "int|false"), ("$safe", "int")]);

    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///max_min_array_key.php";
    let mut checked = 0;
    for (line, text) in content.lines().enumerate() {
        let Some(col) = text.find("$result;") else {
            continue;
        };
        let hover = hover_at(&backend, uri, content, line as u32, col as u32 + 1)
            .unwrap_or_else(|| panic!("no hover on line {line}"));
        let HoverContents::Markup(markup) = &hover.contents else {
            panic!("Expected MarkupContent");
        };
        let got = markup
            .value
            .lines()
            .find_map(|l| l.split_once(" = ").map(|(_, ty)| ty.trim().to_string()))
            .unwrap_or_else(|| panic!("no assignment in hover on line {line}: {}", markup.value));
        assert_eq!(
            &got, "non-empty-array<int, string>",
            "$result on line {line}"
        );
        checked += 1;
    }
    assert_eq!(
        checked, 2,
        "expected to check both the function and the method"
    );
}

/// `var_export()` renders to a string only for the `$return = true` form; the
/// default prints and hands back nothing at all.
#[test]
fn var_export_returns_a_string_only_when_asked_to() {
    let content = r#"<?php
function probe(mixed $value, bool $capture): void {
    $printed = var_export($value);
    $rendered = var_export($value, true);
    $notRendered = var_export($value, false);
    $unknown = var_export($value, $capture);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$printed", "null"),
            ("$rendered", "string"),
            ("$notRendered", "null"),
            ("$unknown", "string|null"),
        ],
    );
}

/// `mb_internal_encoding()` and `version_compare()` are both two functions
/// wearing one name: the reader and the writer, and the comparator and the
/// predicate. Which one a call gets is decided by the optional argument.
#[test]
fn getter_setter_builtins_follow_their_optional_argument() {
    let content = r#"<?php
function probe(string $a, string $b, ?string $op, ?string $enc): void {
    $current = mb_internal_encoding();
    $changed = mb_internal_encoding('UTF-8');
    $eitherEncoding = mb_internal_encoding($enc);
    $ordering = version_compare($a, $b);
    $satisfied = version_compare($a, $b, '>=');
    $eitherCompare = version_compare($a, $b, $op);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$current", "string"),
            ("$changed", "bool"),
            ("$eitherEncoding", "string|bool"),
            ("$ordering", "int"),
            ("$satisfied", "bool"),
            ("$eitherCompare", "int|bool"),
        ],
    );
}

/// The `scanf` family collects into an array or reports how many
/// by-reference targets it filled, decided by whether any were passed. The
/// deciding parameter is the variadic itself, so presence is the whole
/// question — the first target's own type says nothing.
#[test]
fn scanf_family_follows_its_out_parameters() {
    let content = r#"<?php
function probe(string $text, string $format, $stream): void {
    $collected = sscanf($text, $format);
    $assigned = sscanf($text, $format, $day, $month);
    $collectedFromFile = fscanf($stream, $format);
    $assignedFromFile = fscanf($stream, $format, $field);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$collected", "array|null"),
            ("$assigned", "int"),
            ("$collectedFromFile", "array|false|null"),
            ("$assignedFromFile", "int|false"),
        ],
    );
}

/// A `range()` of integers stays integral, and one fractional bound makes
/// every element a float. A bound that cannot be typed keeps the union both
/// answers live in, and a string bound still walks the character range.
#[test]
fn range_elements_follow_all_of_its_bounds() {
    let content = r#"<?php
function probe(string $text, mixed $unknown): void {
    $ints = range(1, 10);
    $steppedInts = range(0, 10, 2);
    $floats = range(1.0, 2.0);
    $fractionalStep = range(0, 1, 0.25);
    $chars = range('a', 'z');
    $either = range($unknown, $unknown);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$ints", "list<int>"),
            ("$steppedInts", "list<int>"),
            ("$floats", "list<float>"),
            ("$fractionalStep", "list<float>"),
            ("$chars", "list<string>"),
            ("$either", "list<string>|list<int|float>"),
        ],
    );
}

/// `array_reduce()` only answers `null` for the one case that produces it: an
/// empty array with no initial value. Seeding the accumulator rules it out.
#[test]
fn array_reduce_drops_null_when_it_is_seeded() {
    let content = r#"<?php
/** @param list<int> $numbers */
function probe(array $numbers): void {
    $seeded = array_reduce($numbers, static fn (int $carry, int $n): int => $carry + $n, 0);
    $unseeded = array_reduce($numbers, static fn (?int $carry, int $n): int => (int) $carry + $n);
}
"#;
    assert_assigned_types(content, &[("$seeded", "int"), ("$unseeded", "int|null")]);
}

/// A string builtin handed literals produces a literal, one per
/// alternative the subject can be. The stub's bare `string` is what the
/// function can return in general, not what this call returns, and a
/// caller checking the result against a literal union needs the
/// difference.
#[test]
fn string_builtins_fold_the_literals_they_are_handed() {
    let content = r#"<?php
function probe(bool $flag, string $unknown): void {
    $kind = $flag ? 'Interface' : 'Trait';
    $lowered = strtolower($kind);
    $shouted = strtoupper('hello');
    $titled = ucfirst('hello');
    $trimmed = trim('  padded  ');
    $swapped = str_replace('_', '-', 'a_b');
    $unfoldable = strtolower($unknown);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$lowered", "'interface'|'trait'"),
            ("$shouted", "'HELLO'"),
            ("$titled", "'Hello'"),
            ("$trimmed", "'padded'"),
            ("$swapped", "'a-b'"),
            ("$unfoldable", "string"),
        ],
    );
}

/// A constant's value in the stubs describes the machine the stubs came
/// from, so folding a builtin over one would turn an
/// environment-dependent answer into a literal the engine then treats as
/// certain. The declared type is the honest answer there.
#[test]
fn a_string_builtin_over_a_constant_keeps_its_declared_type() {
    let content = r#"<?php
function probe(string $path): void {
    $normalised = str_replace(DIRECTORY_SEPARATOR, '/', $path);
    $os = strtolower(PHP_OS);
}
"#;
    assert_assigned_types(content, &[("$normalised", "string"), ("$os", "string")]);
}

/// `pow()`'s `object` branch belongs to the operator-overloading extensions
/// (GMP, BCMath); two numbers can only produce a number. Only an operand
/// that *is* one of those objects brings the branch back — an operand nobody
/// typed is no evidence for it, and unioning the branch back in for every
/// such call would report an `object` where the code returns a number.
#[test]
fn pow_reports_an_object_only_for_an_operand_that_is_one() {
    let content = r#"<?php
function probe(int $n, float $f, mixed $anything, \GMP $gmp): void {
    $ints = pow(2, 3);
    $mixedNumeric = pow($n, $f);
    $untyped = pow($anything, 2);
    $overloaded = pow($gmp, 2);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$ints", "int|float"),
            ("$mixedNumeric", "int|float"),
            ("$untyped", "int|float"),
            ("$overloaded", "object"),
        ],
    );
}

/// `ini_get()` reports `false` for a directive that is not set, which the
/// core directives always are. Anything outside that list keeps the union.
#[test]
fn ini_get_drops_false_for_directives_php_always_defines() {
    let content = r#"<?php
function probe(string $option): void {
    $limit = ini_get('memory_limit');
    $timezone = ini_get('date.timezone');
    $precision = ini_get('precision');
    $extensionOption = ini_get('xdebug.mode');
    $either = ini_get($option);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$limit", "string"),
            ("$timezone", "string"),
            ("$precision", "string"),
            ("$extensionOption", "string|false"),
            ("$either", "string|false"),
        ],
    );
}

/// `get_class()` names the class of the object it was handed, and
/// `ReflectionClass::getInterfaceNames()` names classes too. Both are
/// declared as plain strings, which loses the one thing the call establishes.
#[test]
fn class_naming_builtins_keep_their_class_string() {
    let content = r#"<?php
class Widget {}
function probe(Widget $widget, object $anything, \ReflectionClass $reflection): void {
    $exact = get_class($widget);
    $any = get_class($anything);
    $interfaces = $reflection->getInterfaceNames();
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$exact", "class-string<Widget>"),
            ("$any", "class-string<object>"),
            ("$interfaces", "list<class-string>"),
        ],
    );
}

/// `func_get_args()` always returns the arguments as a list.
#[test]
fn func_get_args_returns_a_list() {
    let content = r#"<?php
function probe($x = null): void {
    $args = func_get_args();
}
"#;
    assert_assigned_types(content, &[("$args", "list<mixed>")]);
}
