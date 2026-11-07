# PHPantom — Completion

Dynamic return type handling for built-in functions, stub attribute
extraction, and argument-level intelligence. Items that are about
_type resolution infrastructure_ (generics, narrowing, conditional
types) live in [type-inference.md](type-inference.md).

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## C1. Array functions needing new code paths

**Impact: Medium · Complexity: High**

These functions have return type semantics that don't fit into either
`ARRAY_PRESERVING_FUNCS` (same array type out) or `ARRAY_ELEMENT_FUNCS`
(single element out). Each needs its own mini-resolver (in
`array_func_rules`) or a stub patch. The key-extracting functions
(`array_keys`, `array_search`, `array_key_first`/`_last`, `key`,
`array_find_key`), `array_fill_keys`, and `min`/`max` are already
handled; the table lists what still resolves to a bare `array` or a
loose type.

| Function                             | Return type logic                                              | PHPStan extension                               |
| ------------------------------------ | -------------------------------------------------------------- | ----------------------------------------------- |
| `array_column`                       | Extracts a column from a 2D array, preserving types            | `ArrayColumnFunctionReturnTypeExtension`        |
| `array_combine`                      | Keys from first array arg, values from second                  | `ArrayCombineFunctionReturnTypeExtension`       |
| `array_fill`                         | `array<int, TValue>` preserving the fill value type            | `ArrayFillFunctionReturnTypeExtension`          |
| `array_flip`                         | Swaps key↔value types (today: `array<int\|string, array-key>`) | `ArrayFlipFunctionReturnTypeExtension`          |
| `array_pad`                          | Union of existing value type + pad value type                  | `ArrayPadDynamicReturnTypeExtension`            |
| `array_replace`                      | Merge-like, preserving types from all args                     | `ArrayReplaceFunctionReturnTypeExtension`       |
| `array_change_key_case`              | Preserves value type, transforms key type                      | `ArrayChangeKeyCaseFunctionReturnTypeExtension` |
| `array_intersect_key`                | Preserves first array's types (missing from `ARRAY_PRESERVING_FUNCS`, unlike `array_diff_key`) | `ArrayIntersectKeyFunctionReturnTypeExtension`  |
| `array_rand`                         | Returns key type of the input array (today: `array\|string\|int`) | `ArrayRandFunctionReturnTypeExtension`          |
| `array_count_values`                 | Returns `array<TValue, int>`                                   | `ArrayCountValuesDynamicReturnTypeExtension`    |
| `compact`                            | Builds typed array from variable names                         | `CompactFunctionReturnTypeExtension`            |
| `count` / `sizeof`                   | Returns precise int range based on array size (today: always `int<0, max>`) | `CountFunctionReturnTypeExtension`              |

---

## C3. Go-to-definition for array shape keys via bracket access

**Impact: Low-Medium · Complexity: Medium**

Array shape keys accessed via bracket notation (`$status['code']`)
have no go-to-definition support. The type comes from a
`@phpstan-type` / `@phpstan-import-type` alias or a direct
`@var` / `@return` annotation resolved to
`array{code: int, label: string}`, but Ctrl+Click on the string
key inside `['code']` does nothing.

Object shape properties (`$profile->name` from
`@return object{name: string}`) already jump to the property key
in the docblock. Extending the same approach to bracket-access
array shapes would require detecting the array key context in the
GTD path (similar to array shape completion) and searching for the
key inside the matching `array{…}` annotation.

---

## C7. `class_alias()` support

**Impact: Low-Medium · Complexity: Medium-High**

Resolve `class_alias('OriginalClass', 'AliasName')` so that the alias
name works for completion, go-to-definition, and hover. PHP's
`class_alias()` creates a runtime alias for a class, and many codebases
rely on this for backwards compatibility layers. (Laravel's facade
aliases from `config/app.php` are already resolved separately, in
`virtual_members/laravel/aliases.rs`.)

Today, if a file calls `class_alias('\App\Services\UserService',
'UserService')`, using `UserService` elsewhere produces no completions
and no go-to-definition because PHPantom has no record of the alias.

**Implementation:**

1. **Detect `class_alias()` calls** — during AST extraction (in
   `parser/functions.rs` or a new pass), scan for top-level
   `class_alias(string, string)` calls where both arguments are string
   literals.

2. **Store aliases in the use map** — treat each alias as an implicit
   `use OriginalClass as AliasName` entry. This slots into the existing
   class resolution pipeline: when resolving `AliasName`, the use map
   lookup finds `OriginalClass`, and all existing resolution, completion,
   and definition logic works without changes.

3. **Cross-file aliases** — for aliases defined in autoloaded files
   (e.g. a library's bootstrap or compatibility file), the alias
   mapping needs to be stored in `fqn_uri_index` or a parallel index so
   that it's available project-wide. This is the main effort: deciding
   where to persist the alias data and when to scan for it.

4. **Edge cases** — `class_alias` with a variable or concatenated
   string as an argument is not statically resolvable. Only handle
   literal string arguments. Conditional `class_alias` calls (inside
   `if (!class_exists(...))` guards) are common and should still be
   processed since the alias is expected to be available at analysis
   time.

---

## C8. Filesystem proximity as an affinity tiebreaker

**Impact: Low-Medium · Complexity: Medium**

The affinity table is built from the file's `use` imports and namespace
declaration, which works well when the file already has imports. In
cold-start scenarios (new file, few imports), the affinity table is
sparse and many candidates share the same zero score. Adding a
secondary proximity signal based on the candidate's source file path
would improve ranking in these cases.

The classmap already stores file paths for every autoloaded class.
When two candidates share the same affinity score, prefer the one
whose source file is closer in the directory tree to the file being
edited. This mirrors Phpactor's `SimilarityResultPrioritizer`, which
computes path-segment overlap between the source file and the
candidate file.

**Implementation:**

1. **Compute a proximity score** — given the current file's path and a
   candidate's classmap path, count shared path segments (or use the
   inverse of the differing-segment count). Normalize to a small
   integer range (e.g. 0-99).

2. **Integrate into `class_sort_text`** — add a new dimension after
   `affinity` (the key is currently affinity, demote, gap). This keeps
   it as a tiebreaker within the same affinity bracket rather than
   overriding the namespace-usage signal. Only apply when the affinity
   score is zero or tied.

3. **Pass the current file path** through `ClassCompletionParams` and
   into `ClassItemCtx` so it's available during sort-text construction.

---

## C11. Smarter member ordering after `->` / `::`

**Impact: Medium · Complexity: High**

Members after `->` and `::` are now sorted by kind (constants and
`::class` first, then properties, then methods, with implemented magic
methods pushed below regular methods so `__invoke`/`__toString`/etc.
don't sit at the top of the method list from their leading
underscores), and alphabetically within each group
(`src/completion/builder.rs`, `kind_sort_tier` / `magic_sort_tier`).
That's still not always helpful: large classes (Laravel Eloquent
models, Symfony form builders, PHPUnit test cases) can have hundreds of
members, and the method the user most likely wants is buried
alphabetically among inherited helpers.

This is a longer-term goal that needs design work before implementation.
Possible ranking signals still to explore, on top of the kind/magic
tiering already in place:

- **Visibility**: public members above protected when accessed from
  outside the class hierarchy
- **Declaration origin**: own members above inherited, inherited above
  trait-mixed, trait-mixed above mixin-provided
- **Usage frequency**: members used elsewhere in the current file or
  project rank higher (requires some form of usage tracking)
- **Deprecation**: deprecated members demoted to the bottom
- **Name prefix match**: when the user has typed a partial member name,
  apply match-quality tiering (exact > prefix > substring) similar to
  class-name completion

The right combination of these signals (and their relative weights)
needs experimentation. A next step could be adding declaration origin
on top of the existing kind tiering, which requires no new data and is
straightforward to implement.

---

## C10. Deprecation markers on class-name completions from all sources

**Impact: Low · Complexity: Medium**

Both class-name completion passes (`fqn_uri_index` and stubs) mark a
candidate deprecated when its `ClassInfo` is already loaded
(`find_class_in_uri_classes_index` in `class_completion/mod.rs`),
whatever its source. Classes that have not been loaded yet show no
strikethrough even when they carry `@deprecated`.

For classmap entries, a lightweight byte-level scan of the first
docblock in the file (similar to `detect_stub_class_kind`) could detect
`@deprecated` without a full parse. For stubs, the source is already
in memory and could be scanned cheaply. For fqn_uri_index entries, the
deprecation flag could be stored alongside the file path when the class
is first indexed.

---

## C12. The implicit `$value` of a `set` hook is not offered by variable completion

**Impact: Low · Complexity: Medium**

A `set` hook that declares no parameter list still receives the assigned
value as `$value`:

```php
class Item {
    public Price $label {
        set {
            $this->stored = $value->format();  // `$value` resolves
        }
    }
}
```

The type engine seeds `$value` from the property's declared type, so
hover, go-to-definition, and diagnostics all read it correctly. Typing
`$va` inside the hook body does not offer it, though, because variable
completion is driven by `SymbolMap::var_defs` and there is no `$value`
token in the source to anchor a `VarDefSite` to. A hook that spells the
parameter out (`set(Price $value)`) is offered as usual.

Anchoring a synthetic def site on the `set` keyword is not enough on its
own: `var_defs` offsets are also read as real source ranges by find
references, rename, document highlight, and semantic tokens, so a def
site pointing at a keyword would produce a bogus highlight and a rename
edit that corrupts the hook. The fix needs a def kind those consumers
skip (say `VarDefKind::ImplicitHookValue`) while variable completion
includes it.

---

## C13. `self::`/`static::` inside a `@require-extends` trait does not see the required class's static members

**Impact: Low-Medium · Complexity: Medium**

Found while porting Mago's `issue_1064.php` (Test Porting Phase 4A).
`$this->` completion inside a trait carrying `@phpstan-require-extends`/
`@psalm-require-extends` already offers the required class's instance
methods and properties (`test_completion_require_extends_this_members_*`
in `completion_traits.rs`), via `trait_this_bounds`/
`extend_this_with_trait_bounds` in `type_engine/trait_context.rs`. Static
access does not get the same treatment:

```php
class Foo {
    const int FOO = 42;
}

/** @psalm-require-extends Foo */
trait Bar {
    public function foo(): int {
        return static::   // completion offers nothing from Foo
    }
}
```

Every caller of `trait_this_bounds`/`extend_this_with_trait_bounds` is on
a `$this`-context resolution path (`forward_walk/param_seeding.rs`,
`forward_walk/callable_inference.rs`, `resolver/variable_subjects.rs`, and
`rhs_resolution/property_access.rs`). None of them run for a bare
`self::`/`static::` completion inside a trait body, so the required
class's constants and static properties/methods never enter the
candidate list there.

**Fix:** `SubjectExpr::SelfKw | StaticKw` goes straight to
`resolve_self_static_class` (`type_engine/resolver/mod.rs`) with no
trait-bound extension. Feed it through the same
`@require-extends`/`@require-implements` bound the `$this->` path
already consults, rather than adding a second lookup of the tag.
