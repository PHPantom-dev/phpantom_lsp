//! Integration tests for the array builtins that answer in terms of their
//! input's generics.
//!
//! phpstorm-stubs declare these as returning a bare `array`, an `int[]|string[]`
//! union, or `string|int|false`, because a signature without generics cannot
//! say "the key type of whatever you passed". Each test here pins one function
//! to the type its argument implies, and checks that an argument with nothing
//! to say (a bare `array`) still gets the declared type.
//!
//! The last test covers the binding machinery the rest of them rest on: which
//! alternative of a union `@param` a `@template` binds from.

use crate::common::{assert_assigned_types, create_test_backend_with_full_stubs, type_at_marker};

/// Assert the type of each assignment in `content`, keyed by the variable it
/// assigns to. Line numbers are found by scanning for `$name = `.
/// The key-reading builtins report the input's *key* type. The stubs spell
/// these out as `int[]|string[]` and `string|int|null`, which then fails
/// against a declared `array<string>` on the wrong branch.
#[test]
fn key_readers_report_the_input_key_type() {
    let content = r#"<?php
class User {}
/**
 * @param array<string, User> $byName
 * @param list<User> $users
 */
function probe(array $byName, array $users, array $bare): void {
    $names = array_keys($byName);
    $indices = array_keys($users);
    $first = array_key_first($byName);
    $last = array_key_last($byName);
    $cursor = key($byName);
    $unknown = array_keys($bare);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$names", "list<string>"),
            ("$indices", "list<int>"),
            ("$first", "string|null"),
            ("$last", "string|null"),
            ("$cursor", "string|null"),
            // A bare `array` says nothing about its keys beyond PHP's own
            // rule that a key is an `array-key`.
            ("$unknown", "list<array-key>"),
        ],
    );
}

/// `array_search` hands back a *key*, so the `int` half of the stub's
/// `string|int|false` is impossible for a string-keyed array.
#[test]
fn array_search_reports_the_input_key_type() {
    let content = r#"<?php
/**
 * @param array<string, int> $byName
 * @param list<string> $names
 */
function probe(array $byName, array $names): void {
    $key = array_search(1, $byName);
    $index = array_search('a', $names);
}
"#;
    assert_assigned_types(
        content,
        &[("$key", "string|false"), ("$index", "int|false")],
    );
}

/// `array_values` renumbers, so it keeps the element type and drops the key
/// type — it is a `list<V>`, never the `array<K, V>` it was handed.
#[test]
fn array_values_renumbers_to_a_list() {
    let content = r#"<?php
class User {}
/**
 * @param array<string, User> $byName
 * @param array<string, int> $counts
 */
function probe(array $byName, array $counts): void {
    $users = array_values($byName);
    $numbers = array_values($counts);
}
"#;
    assert_assigned_types(
        content,
        &[("$users", "list<User>"), ("$numbers", "list<int>")],
    );
}

/// The type-preserving family keeps its element type whether or not that
/// element is a scalar. A `list<string>` is as worth preserving as a
/// `list<User>`; both used to fall back to a bare `array`.
#[test]
fn preserving_builtins_keep_scalar_elements() {
    let content = r#"<?php
class User {}
/**
 * @param list<string> $names
 * @param list<User> $users
 */
function probe(array $names, array $users, array $bare): void {
    $unique = array_unique($names);
    $slice = array_slice($names, 0, 2);
    $merged = array_merge($names, $names);
    $reversed = array_reverse($names);
    $objects = array_reverse($users);
    $unknown = array_unique($bare);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$unique", "list<string>"),
            ("$slice", "list<string>"),
            ("$merged", "list<string>"),
            ("$reversed", "list<string>"),
            ("$objects", "list<User>"),
            ("$unknown", "array"),
        ],
    );
}

/// The element-extracting family has the same scalar blind spot:
/// `array_pop(list<string>)` is a `string`, not `mixed`.
#[test]
fn element_extractors_keep_scalar_elements() {
    let content = r#"<?php
class User {}
/**
 * @param list<string> $names
 * @param list<User> $users
 */
function probe(array $names, array $users): void {
    $popped = array_pop($names);
    $shifted = array_shift($names);
    $cursor = current($names);
    $object = array_pop($users);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$popped", "string"),
            ("$shifted", "string"),
            ("$cursor", "string"),
            ("$object", "User"),
        ],
    );
}

/// `array_filter` with no callback keeps exactly the truthy members, so the
/// element type drops `null`. A callback the analysis cannot read leaves the
/// element type alone, since anything it approves of could be in there.
/// Either way the surviving entries keep their original keys, so a filtered
/// `list` comes back renumbered as `array<int, T>`.
#[test]
fn array_filter_without_a_callback_drops_falsy_members() {
    let content = r#"<?php
class User {}
/**
 * @param array<string, string|null> $maybe
 * @param list<User|null> $users
 * @param list<string> $plain
 */
function probe(array $maybe, array $users, array $plain, callable $cb): void {
    $kept = array_filter($maybe);
    $present = array_filter($users);
    $chosen = array_filter($maybe, $cb);
    $unchanged = array_filter($plain);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$kept", "array<string, string>"),
            ("$present", "array<int, User>"),
            ("$chosen", "array<string, string|null>"),
            ("$unchanged", "array<int, string>"),
        ],
    );
}

/// `array_filter` keeps the key of every entry it keeps, so filtering a `list`
/// leaves gaps in the numbering and the result is no longer a `list`. The
/// renumbering functions around it are the ones that rebuild the promise, and
/// a filter may drop every entry, so a `non-empty-` refinement goes too.
#[test]
fn array_filter_drops_the_list_promise_its_input_carried() {
    let content = r#"<?php
/**
 * @param list<int> $values
 * @param non-empty-list<int> $some
 * @param non-empty-array<string, int> $rows
 * @param array<string, int> $keyed
 */
function probe(array $values, array $some, array $rows, array $keyed, callable $cb): void {
    $filtered = array_filter($values, $cb);
    $truthy = array_filter($values);
    $from_non_empty = array_filter($some, $cb);
    $mapped = array_filter($rows, $cb);
    $unchanged = array_filter($keyed, $cb);
    $shape = array_filter([1, 2, 3], $cb);
    $renumbered = array_values(array_filter($values, $cb));
    $preserved = array_unique($values);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$filtered", "array<int, int>"),
            ("$truthy", "array<int, int>"),
            ("$from_non_empty", "array<int, int>"),
            ("$mapped", "array<string, int>"),
            ("$unchanged", "array<string, int>"),
            ("$shape", "array<int, 1|2|3>"),
            ("$renumbered", "list<int>"),
            ("$preserved", "list<int>"),
        ],
    );
}

/// A `?bool` entry sitting alongside other nullable entries in the same
/// array shape must not stop `array_filter()` from stripping `null` off
/// its siblings: each entry's truthy half is worked out on its own, not
/// only when the whole merged value type happens to be a single nullable.
#[test]
fn array_filter_without_a_callback_strips_null_from_every_entry_sharing_a_bool() {
    let content = r#"<?php
function probe(?int $w, ?bool $a, ?string $s, ?DateTime $d): void {
    $arr = ['w' => $w, 'a' => $a, 's' => $s, 'd' => $d];
    $filtered = array_filter($arr);
}
"#;
    assert_assigned_types(
        content,
        &[("$filtered", "array<string, int|true|string|DateTime>")],
    );
}

/// `array_sum`/`array_product` are declared `int|float` for PHP's numeric
/// promotion, but an all-`int` array can only sum to an `int`. A union that
/// really can go either way keeps both.
#[test]
fn numeric_folds_narrow_on_an_all_int_array() {
    let content = r#"<?php
/**
 * @param array<int> $ints
 * @param list<float> $floats
 * @param list<int|float> $either
 * @param list<string> $strings
 */
function probe(array $ints, array $floats, array $either, array $strings, array $bare): void {
    $total = array_sum($ints);
    $product = array_product($ints);
    $money = array_sum($floats);
    $mixed = array_sum($either);
    $numeric = array_sum($strings);
    $unknown = array_sum($bare);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$total", "int"),
            ("$product", "int"),
            ("$money", "float"),
            ("$mixed", "int|float"),
            ("$numeric", "int|float"),
            ("$unknown", "int|float"),
        ],
    );
}

/// A `@template` named in more than one alternative of a union `@param`
/// binds from the alternative the argument's own shape matches, not from
/// whichever one happens to be written first.
#[test]
fn a_template_binds_through_a_union_param() {
    let content = r#"<?php
/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection {}

/**
 * @template TKey of array-key
 * @template TValue
 * @param Collection<TKey, TValue>|array<TKey, TValue> $items
 * @return array<TKey, TValue>
 */
function pick($items): array { return []; }

/**
 * @param array<string, int> $rows
 * @param list<string> $names
 */
function probe(array $rows, array $names): void {
    $picked = pick($rows);
    $listed = pick($names);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$picked", "array<string, int>"),
            ("$listed", "array<int, string>"),
        ],
    );
}

/// `array_filter` in one of the two modes that hand the callback the key
/// keeps only the keys the callback approves of, so what its body asserts
/// about them describes the result's key type.
#[test]
fn array_filter_narrows_the_key_type_from_its_callback() {
    let content = r#"<?php
/**
 * @param array<string|int, string> $data
 */
function probe(array $data): void {
    $keyed = array_filter($data, fn (string|int $k): bool => is_string($k), ARRAY_FILTER_USE_KEY);
    $both = array_filter($data, fn (string $v, string|int $k): bool => is_string($k), ARRAY_FILTER_USE_BOTH);
    $closure = array_filter($data, function ($k) { return is_int($k); }, ARRAY_FILTER_USE_KEY);
    $negated = array_filter($data, fn ($k) => !is_int($k), ARRAY_FILTER_USE_KEY);
    $conjunction = array_filter($data, fn ($k) => is_string($k) && $k !== '', ARRAY_FILTER_USE_KEY);
    $named = array_filter($data, 'is_string', ARRAY_FILTER_USE_KEY);
    $inline = array_keys(array_filter($data, fn ($k) => is_string($k), ARRAY_FILTER_USE_KEY));
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$keyed", "array<string, string>"),
            ("$both", "array<string, string>"),
            ("$closure", "array<int, string>"),
            ("$negated", "array<string, string>"),
            ("$conjunction", "array<string, string>"),
            ("$named", "array<string, string>"),
            ("$inline", "list<string>"),
        ],
    );
}

/// In the two modes that hand the callback the value, what its body asserts
/// about it describes the result's element type — the whole point of
/// `array_filter($items, fn ($i) => $i !== null)`.
#[test]
fn array_filter_narrows_the_element_type_from_its_callback() {
    let content = r#"<?php
class User {}
class Admin extends User {}
/**
 * @param array<string, string|null> $maybe
 * @param list<User|Admin> $users
 * @param array<int|string> $mixed
 */
function probe(array $maybe, array $users, array $mixed): void {
    $present = array_filter($maybe, fn ($v) => $v !== null);
    $guarded = array_filter($maybe, fn ($v) => !is_null($v));
    $named = array_filter($mixed, 'is_int');
    $closure = array_filter($mixed, function ($v) { return is_string($v); });
    $instances = array_filter($users, fn ($v) => $v instanceof Admin);
    $both = array_filter($maybe, fn ($v, $k) => $v !== null, ARRAY_FILTER_USE_BOTH);
    $inline = array_values(array_filter($maybe, fn ($v) => $v !== null));
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$present", "array<string, string>"),
            ("$guarded", "array<string, string>"),
            ("$named", "array<int>"),
            ("$closure", "array<string>"),
            ("$instances", "array<int, Admin>"),
            ("$both", "array<string, string>"),
            ("$inline", "list<string>"),
        ],
    );
}

/// `get_class($v) !== Foo::class` is not `!($v instanceof Foo)`: it rules out
/// exactly one class, so a subclass, whose `get_class()` names the subclass,
/// passes the comparison and survives the filter.
#[test]
fn array_filter_keeps_subclasses_past_a_negated_exact_class_check() {
    let content = r#"<?php
class Animal {}
class Dog extends Animal {}
class Puppy extends Dog {}
class Cat extends Animal {}
/**
 * @param list<Dog|Puppy|Cat> $pets
 */
function probe(array $pets): void {
    $not_exactly_a_dog = array_filter($pets, fn ($v) => get_class($v) !== Dog::class);
    $not_a_dog_at_all = array_filter($pets, fn ($v) => !($v instanceof Dog));
    $exactly_a_dog = array_filter($pets, fn ($v) => get_class($v) === Dog::class);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$not_exactly_a_dog", "array<int, Puppy|Cat>"),
            ("$not_a_dog_at_all", "array<int, Cat>"),
            ("$exactly_a_dog", "array<int, Dog>"),
        ],
    );
}

/// An `instanceof` check on a union member that already names the checked
/// class keeps that member as it was written, type arguments included — the
/// member is the more specific of the two, so replacing it with the bare
/// class would throw away what the chain after the filter needs.
#[test]
fn array_filter_keeps_the_type_arguments_of_a_narrowed_union_member() {
    let content = r#"<?php
class User {}
/**
 * @template T
 */
class Collection {
    /** @return T */
    public function first() {}
}
/**
 * @param list<Collection<User>|string> $items
 */
function probe(array $items): void {
    $collections = array_filter($items, fn ($v) => $v instanceof Collection);
}
"#;
    assert_assigned_types(content, &[("$collections", "array<int, Collection<User>>")]);
}

/// Only the strict comparison proves a value is null. `!($v != null)` is the
/// loose `$v == null` spelled backwards, and that also admits `''`, `0` and
/// `[]`, so it narrows nothing.
#[test]
fn array_filter_does_not_read_a_negated_loose_null_check_as_a_strict_one() {
    let content = r#"<?php
/**
 * @param list<string|null> $xs
 */
function probe(array $xs): void {
    $loose = array_filter($xs, fn ($v) => !($v != null));
    $strict = array_filter($xs, fn ($v) => !($v !== null));
    $present = array_filter($xs, fn ($v) => !($v === null));
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$loose", "array<int, string|null>"),
            ("$strict", "array<int, null>"),
            ("$present", "array<int, string>"),
        ],
    );
}

/// A callback handed only the key says nothing about the values, and a
/// callback that admits every value it could receive leaves the element type
/// as it found it.
#[test]
fn array_filter_keeps_the_element_type_a_callback_says_nothing_about() {
    let content = r#"<?php
/**
 * @param array<string, string|null> $maybe
 */
function probe(array $maybe, callable $cb): void {
    $key_mode = array_filter($maybe, fn ($k) => is_string($k), ARRAY_FILTER_USE_KEY);
    $opaque = array_filter($maybe, $cb);
    $unrelated = array_filter($maybe, fn ($v) => strlen((string) $v) > 2);
    $every = array_filter($maybe, fn ($v) => is_string($v) || is_null($v));
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$key_mode", "array<string, string|null>"),
            ("$opaque", "array<string, string|null>"),
            ("$unrelated", "array<string, string|null>"),
            ("$every", "array<string, string|null>"),
        ],
    );
}

/// A callback that proves nothing about the key it is handed leaves the
/// key type alone, and so does one whose key never reaches it.
#[test]
fn array_filter_keeps_the_key_type_a_callback_says_nothing_about() {
    let content = r#"<?php
/**
 * @param array<string|int, string> $data
 */
function probe(array $data): void {
    $plain = array_filter($data, fn (string $v): bool => $v !== '');
    $value_mode = array_filter($data, fn (string $v): bool => is_string($v));
    $unrelated = array_filter($data, fn ($k) => strlen((string) $k) > 2, ARRAY_FILTER_USE_KEY);
    $either = array_filter($data, fn ($v, $k) => is_int($k) || is_string($k), ARRAY_FILTER_USE_BOTH);
    $branching = array_filter($data, function ($k) { if ($k === 0) { return true; } return is_string($k); }, ARRAY_FILTER_USE_KEY);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$plain", "array<string|int, string>"),
            ("$value_mode", "array<string|int, string>"),
            ("$unrelated", "array<string|int, string>"),
            ("$either", "array<string|int, string>"),
            ("$branching", "array<string|int, string>"),
        ],
    );
}

/// `array<T>` and `T[]` name a value type and leave the key domain open, so
/// the callback narrows every key PHP permits — the same result the spelled
/// out `array<string|int, T>` gets. A `list<T>` does promise `int` keys, so
/// a callback asking for string keys has nothing to keep and the element
/// type stands, over the `array<int, T>` a filtered list always decays to.
#[test]
fn array_filter_narrows_the_open_key_domain_of_a_shorthand_array() {
    let content = r#"<?php
/**
 * @param array<string> $shorthand
 * @param string[] $slice
 * @param list<string> $sequential
 */
function probe(array $shorthand, array $slice, array $sequential): void {
    $from_shorthand = array_filter($shorthand, fn ($k) => is_string($k), ARRAY_FILTER_USE_KEY);
    $from_slice = array_filter($slice, fn ($k) => is_string($k), ARRAY_FILTER_USE_KEY);
    $from_list = array_filter($sequential, fn ($k) => is_string($k), ARRAY_FILTER_USE_KEY);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$from_shorthand", "array<string, string>"),
            ("$from_slice", "array<string, string>"),
            ("$from_list", "array<int, string>"),
        ],
    );
}

/// A narrowed key type survives PHP's array union: `+` combines what each
/// operand actually carries, so merging two string-keyed arrays stays
/// string-keyed however the operands were spelled.
#[test]
fn the_array_union_operator_keeps_a_narrowed_key_type() {
    let content = r#"<?php
/**
 * @param array<string> $raw
 * @return array<string, string>
 */
function shared(array $raw): array { return []; }

/**
 * @param array<string> $raw
 */
function probe(array $raw): void {
    $narrowed = array_filter($raw, fn ($k) => is_string($k), ARRAY_FILTER_USE_KEY);
    $left = shared($raw) + $narrowed;
    $right = $narrowed + shared($raw);
    $chained = shared($raw) + $narrowed + shared($raw);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$narrowed", "array<string, string>"),
            ("$left", "array<string, string>"),
            ("$right", "array<string, string>"),
            ("$chained", "array<string, string>"),
        ],
    );
}

/// A method call handed straight to a key-reading builtin binds the same
/// key type a local holding its result would. The forward walker seeds a
/// subject key for the call before it can answer it, and that empty entry
/// used to be read as `mixed` — an answer wide enough to stop the argument
/// ever reaching the call resolver that knows its return type.
#[test]
fn a_key_reader_binds_from_a_method_call_argument() {
    let content = r#"<?php
class Registry {
    /** @return array<string, string> */
    public function templates(): array { return []; }

    /** @return array<int, string> */
    public function rows(): array { return []; }
}

function probe(Registry $registry): void {
    $names = array_keys($registry->templates());
    $indices = array_keys($registry->rows());
    $values = array_values($registry->templates());
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$names", "list<string>"),
            ("$indices", "list<int>"),
            ("$values", "list<string>"),
        ],
    );
}

/// An index PHP only produces by computing it (`$m[$line + 1]`) types the
/// written element just as a plain variable index does. Resolving it
/// through the narrower expression resolver left arithmetic unanswered and
/// widened the whole key domain to `array-key`.
#[test]
fn a_computed_index_keeps_the_key_type_it_computes() {
    let content = r#"<?php
function probe(string $contents, int $start): void {
    $lines = [];
    foreach (explode("\n", $contents) as $index => $line) {
        $lines[$index + 1] = $line;
    }
    $offsets = $lines;

    $pairs = [];
    $pairs[$start] = 'a';
    $pairs[$start + 1] = 'b';
    $doubled = $pairs;

    $cursor = $start;
    $seen = [];
    $seen[++$cursor] = 'c';
    $counted = $seen;
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$offsets", "array<int, string>"),
            ("$doubled", "non-empty-array<int, string>"),
            ("$counted", "non-empty-array<int, string>"),
        ],
    );
}

/// The array-function rules are keyed on the bare function name, but a call
/// written `\array_sum($x)` reaches them spelled with the leading namespace
/// separator. Writing the separator used to disable every one of them.
#[test]
fn a_fully_qualified_call_still_gets_the_array_rules() {
    let content = r#"<?php
class User {}
/**
 * @param list<int> $counts
 * @param list<User> $users
 */
function probe(array $counts, array $users): void {
    $total = \array_sum($counts);
    $last = \array_pop($users);
    $kept = \array_filter($users);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$total", "int"),
            ("$last", "User"),
            ("$kept", "array<int, User>"),
        ],
    );
}

/// `array_chunk()` is the one splitter that adds a level of nesting rather
/// than rearranging entries, so its elements are arrays of the input's
/// elements. Grouping it with the type-preserving functions handed back the
/// input's element type and reported each chunk as a single entry.
#[test]
fn array_chunk_nests_the_elements_it_groups() {
    let content = r#"<?php
/**
 * @param array<int, string> $ids
 * @param array<string, int> $byName
 */
function probe(array $ids, array $byName): void {
    $batches = array_chunk($ids, 500);
    $keyed = array_chunk($byName, 10, true);
    $renumbered = array_chunk($byName, 10, false);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$batches", "list<list<string>>"),
            ("$keyed", "list<array<string, int>>"),
            ("$renumbered", "list<list<int>>"),
        ],
    );
}

/// `array_key_first`/`array_key_last`/`key` are stubbed `TKey|null` because
/// an empty array has no key to report. An argument that proves it has
/// entries rules the `null` out; one that does not keeps it.
#[test]
fn the_key_readers_drop_null_for_a_non_empty_array() {
    let content = r#"<?php
/**
 * @param array<int, float> $weights
 * @param non-empty-array<string, int> $tallies
 */
function probe(array $weights, array $tallies): void {
    $maybe = array_key_last($weights);
    assert($weights !== []);
    $proven = array_key_last($weights);
    $declared = array_key_first($tallies);
    $current = key($tallies);
    $literal = array_key_first(['a' => 1, 'b' => 2]);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$maybe", "int|null"),
            ("$proven", "int"),
            ("$declared", "string"),
            ("$current", "string"),
            ("$literal", "string"),
        ],
    );
}

/// `array_map()` resolves its element type from the callback's return, which
/// used to mean an inline closure only. A callable string names a function
/// whose declared return says the same thing, and either way the single-array
/// form keeps the input's keys instead of renumbering them into a list.
#[test]
fn array_map_reads_a_named_callback_and_keeps_the_input_keys() {
    let content = r#"<?php
/**
 * @param array<int, string> $rows
 * @param array<string, string> $byName
 * @param list<string> $lines
 */
function probe(array $rows, array $byName, array $lines): void {
    $named = array_map('intval', $rows);
    $inline = array_map(fn (string $s): int => (int) $s, $rows);
    $keyed = array_map('intval', $byName);
    $sequential = array_map('intval', $lines);
    $zipped = array_map('str_repeat', $lines, $lines);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$named", "array<int, int>"),
            ("$inline", "array<int, int>"),
            ("$keyed", "array<string, int>"),
            ("$sequential", "list<int>"),
            ("$zipped", "list<string>"),
        ],
    );
}

/// A callback parameter is bound from one element of the array it is handed,
/// including when the argument is a union of array shapes: a `@template`
/// that cannot read an element out of a container binds nothing rather than
/// binding the container itself.
#[test]
fn a_callback_parameter_binds_an_element_of_a_shape_union() {
    let content = r#"<?php
class Lexer {
    public function getLabel(int $token): string { return ''; }
}
class RichParser {
    private const TOKEN_A = 3;
    private const TOKEN_B = 2;
    private Lexer $lexer;

    /** @param array<string, Lexer> $opaque */
    public function probe(bool $cond, array $opaque): void
    {
        $expected = $cond ? [self::TOKEN_A] : [self::TOKEN_A, self::TOKEN_B];
        array_map(fn ($token) => $this->lexer->getLabel(/*TOKEN*/$token), $expected);
        array_map(fn ($lexer) => /*OPAQUE*/$lexer, $opaque);
    }
}
"#;
    let backend = create_test_backend_with_full_stubs();
    let uri = "file:///array_func_generics_shape_union.php";
    backend.update_ast(uri, content);
    // Both arms are literal, so the element type is the values themselves
    // rather than the `int` they widen to.
    assert_eq!(type_at_marker(&backend, uri, content, "TOKEN"), "3|2");
    assert_eq!(type_at_marker(&backend, uri, content, "OPAQUE"), "Lexer");
}

/// `array_merge` concatenates its arguments rather than rearranging one of
/// them, so every argument contributes to the element type. Reading only the
/// first is what left the accumulator idiom (`$out = []; … $out =
/// array_merge($out, $more);`) permanently typed as the empty array it
/// started as, which cost every read off it — `$out[$i]->m()` included — its
/// type.
#[test]
fn array_merge_unions_every_argument() {
    let content = r#"<?php
class User {}
class Order {}
/**
 * @param list<User> $users
 * @param list<Order> $orders
 * @param array<string, User> $byName
 */
function probe(array $users, array $orders, array $byName): void {
    $seeded = array_merge([], $users);
    $both = array_merge($users, $orders);
    $three = array_merge($users, $orders, $byName);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$seeded", "list<User>"),
            ("$both", "list<User|Order>"),
            ("$three", "array<int|string, User|Order>"),
        ],
    );
}

/// The keys follow PHP's own two rules: an integer key is renumbered as the
/// entry is appended, a string key is carried over. So an all-integer merge
/// is a `list`, an all-string one keeps its `string` keys, and a mix carries
/// both.
///
/// An argument that names only its value type (`array<T>`, `T[]`) promises
/// nothing about its keys, and the result says just as little.
#[test]
fn array_merge_keys_follow_php_renumbering() {
    let content = r#"<?php
class User {}
class Order {}
/**
 * @param list<User> $users
 * @param array<string, User> $byName
 * @param array<string, Order> $ordersByName
 * @param array<User> $loose
 * @param User[] $shorthand
 */
function probe(array $users, array $byName, array $ordersByName, array $loose, array $shorthand): void {
    $strings = array_merge($byName, $ordersByName);
    $mixed = array_merge($users, $byName);
    $open = array_merge($loose, $users);
    $shorthandOpen = array_merge($shorthand, $byName);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$strings", "array<string, User|Order>"),
            ("$mixed", "array<int|string, User>"),
            ("$open", "array<User>"),
            ("$shorthandOpen", "array<User>"),
        ],
    );
}

/// An argument the rule cannot read could contribute anything, so it declines
/// and leaves the stub's bare `array` standing rather than claim a union that
/// is missing a member. A bare `array` names no element type, and a spread
/// holds the arrays to merge rather than one of them.
#[test]
fn array_merge_declines_on_arguments_it_cannot_read() {
    let content = r#"<?php
class User {}
/**
 * @param list<User> $users
 * @param list<list<User>> $groups
 */
function probe(array $users, array $groups, array $bare): void {
    $withBare = array_merge($users, $bare);
    $spread = array_merge(...$groups);
    $empty = array_merge([], []);
}
"#;
    assert_assigned_types(
        content,
        &[
            ("$withBare", "array"),
            ("$spread", "array"),
            ("$empty", "array"),
        ],
    );
}
