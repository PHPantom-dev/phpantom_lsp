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
(single element out). Each needs its own mini-resolver.

| Function                             | Return type logic                                              | PHPStan extension                               |
| ------------------------------------ | -------------------------------------------------------------- | ----------------------------------------------- |
| `array_keys`                         | Returns `list<TKey>` — extracts the _key_ type, not value type | `ArrayKeysFunctionDynamicReturnTypeExtension`   |
| `array_column`                       | Extracts a column from a 2D array, preserving types            | `ArrayColumnFunctionReturnTypeExtension`        |
| `array_combine`                      | Keys from first array arg, values from second                  | `ArrayCombineFunctionReturnTypeExtension`       |
| `array_fill`                         | `array<int, TValue>` preserving the fill value type            | `ArrayFillFunctionReturnTypeExtension`          |
| `array_fill_keys`                    | Preserves key array type + value type                          | `ArrayFillKeysFunctionReturnTypeExtension`      |
| `array_flip`                         | Swaps key↔value types                                          | `ArrayFlipFunctionReturnTypeExtension`          |
| `array_pad`                          | Union of existing value type + pad value type                  | `ArrayPadDynamicReturnTypeExtension`            |
| `array_replace`                      | Merge-like, preserving types from all args                     | `ArrayReplaceFunctionReturnTypeExtension`       |
| `array_change_key_case`              | Preserves value type, transforms key type                      | `ArrayChangeKeyCaseFunctionReturnTypeExtension` |
| `array_intersect_key`                | Preserves first array's types (dedicated extension)            | `ArrayIntersectKeyFunctionReturnTypeExtension`  |
| `array_search`                       | Returns key type of the haystack array                         | `ArraySearchFunctionDynamicReturnTypeExtension` |
| `array_rand`                         | Returns key type of the input array                            | `ArrayRandFunctionReturnTypeExtension`          |
| `array_count_values`                 | Returns `array<TValue, int>`                                   | `ArrayCountValuesDynamicReturnTypeExtension`    |
| `array_key_first` / `array_key_last` | Returns key type (usually scalar, low completion value)        | `ArrayFirstLastDynamicReturnTypeExtension`      |
| `key`                                | Returns the key type of the array at the internal pointer      | `KeyFunctionDynamicReturnTypeExtension`         |
| `array_find_key`                     | Returns key type (PHP 8.4)                                     | `ArrayFindKeyFunctionReturnTypeExtension`       |
| `compact`                            | Builds typed array from variable names                         | `CompactFunctionReturnTypeExtension`            |
| `count` / `sizeof`                   | Returns precise int range based on array size                  | `CountFunctionReturnTypeExtension`              |
| `min` / `max`                        | Returns union of argument types                                | `MinMaxFunctionReturnTypeExtension`             |

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

## C4. Non-array functions with dynamic return types

**Impact: Low · Complexity: High**

PHPStan also provides dynamic return type extensions for many non-array
functions. These are lower priority because they mostly refine scalar
return types (less impactful for class-based completion).

| Function                                            | Return type logic                                   | PHPStan extension                                  |
| --------------------------------------------------- | --------------------------------------------------- | -------------------------------------------------- |
| `abs`                                               | Preserves int/float return type                     | `AbsFunctionDynamicReturnTypeExtension`            |
| `base64_decode`                                     | `string\|false` based on strict param               | `Base64DecodeDynamicFunctionReturnTypeExtension`   |
| `explode`                                           | `list<string>` / `non-empty-list<string>` / `false` | `ExplodeFunctionDynamicReturnTypeExtension`        |
| `filter_var`                                        | Return type depends on filter constant              | `FilterVarDynamicReturnTypeExtension`              |
| `filter_input`                                      | Same as `filter_var`                                | `FilterInputDynamicReturnTypeExtension`            |
| `filter_var_array` / `filter_input_array`           | Typed array based on filter definitions             | `FilterVarArrayDynamicReturnTypeExtension`         |
| `get_class`                                         | Returns `class-string<T>`                           | `GetClassDynamicReturnTypeExtension`               |
| `get_called_class`                                  | Returns `class-string<static>`                      | `GetCalledClassDynamicReturnTypeExtension`         |
| `get_parent_class`                                  | Returns parent class-string                         | `GetParentClassDynamicFunctionReturnTypeExtension` |
| `gettype`                                           | Returns specific string literal for known types     | `GettypeFunctionReturnTypeExtension`               |
| `get_debug_type`                                    | Returns specific string literal                     | `GetDebugTypeFunctionReturnTypeExtension`          |
| `constant`                                          | Resolves named constant to its type                 | `ConstantFunctionReturnTypeExtension`              |
| `date` / `date_format`                              | Precise string return types                         | `DateFunctionReturnTypeExtension`                  |
| `date_create` / `date_create_immutable`             | `DateTime\|false`                                   | `DateTimeCreateDynamicReturnTypeExtension`         |
| `hash` / `hash_file` / etc.                         | Precise return types                                | `HashFunctionsReturnTypeExtension`                 |
| `sprintf` / `vsprintf`                              | Non-empty-string preservation                       | `SprintfFunctionDynamicReturnTypeExtension`        |
| `preg_split`                                        | `list<string>\|false` based on flags                | `PregSplitDynamicReturnTypeExtension`              |
| `str_split` / `mb_str_split`                        | Non-empty-list                                      | `StrSplitFunctionReturnTypeExtension`              |
| `class_implements` / `class_uses` / `class_parents` | `array<string, string>\|false`                      | `ClassImplementsFunctionReturnTypeExtension`       |

---

## C5. `#[ReturnTypeContract]` parameter-dependent return types

**Impact: Low · Complexity: Medium**

phpstorm-stubs use `#[ReturnTypeContract]` on 4 functions to express
return type narrowing based on a parameter's value or presence. These
functions have no `@phpstan-return` conditional type in their docblocks,
so the narrowing information is only available through the attribute.

**Attribute FQN:** `JetBrains\PhpStorm\Internal\ReturnTypeContract`.
Stub files import it as `TypeContract` via
`use JetBrains\PhpStorm\Internal\ReturnTypeContract as TypeContract;`.
Match by resolving through the `DocblockCtx` use-map and comparing the
last segment of the resolved FQN (`ReturnTypeContract`).

The attribute has four named arguments:

- `true` / `false` — narrows the return type when the annotated boolean
  parameter is `true` or `false`.
- `exists` / `notExists` — narrows the return type when an optional
  variadic parameter is passed or omitted.

```php
// microtime(true) → float, microtime(false) → string
function microtime(
    #[TypeContract(true: "float", false: "string")] bool $as_float = false
): string|float {}

// sscanf with extra args → int|null, without → array|null
function sscanf(
    string $string, string $format,
    #[TypeContract(exists: "int|null", notExists: "array|null")] mixed &...$vars
): array|int|null {}
```

Affected functions: `microtime`, `gettimeofday`, `sscanf`, `fscanf`.
Three of the four already carry a hand-written conditional return type in
`stub_patches`, which is what makes them resolve today; `gettimeofday` is
the one still on its declared union. Reading the attribute would replace
all four hand-written patches with one rule, and cover any function
phpstorm-stubs annotates next.

**Implementation:** When resolving a call to one of these functions,
check whether the annotated parameter was passed (for `exists`/
`notExists`) or matches a literal boolean (for `true`/`false`). Use the
narrowed type from the attribute instead of the declared union return
type. This integrates into the call return type resolution path.

---

## C6. `#[ExpectedValues]` parameter value suggestions

**Impact: Low · Complexity: Medium**

phpstorm-stubs annotate ~62 parameters and return values with
`#[ExpectedValues]` to declare the set of valid constant values or
flags. This could power smarter completions inside function call
arguments by suggesting the valid constants.

**Attribute FQN:** `JetBrains\PhpStorm\ExpectedValues`. Stub files
import it via `use JetBrains\PhpStorm\ExpectedValues;`. Two files alias
it as `EV` (`intl/intl.php` and `ftp/ftp.php`). Match by resolving
through the `DocblockCtx` use-map and comparing the last segment of the
resolved FQN (`ExpectedValues`).

The attribute supports several forms:

- `values: [CONST_A, CONST_B]` — one of the listed values is expected.
- `flags: [FLAG_A, FLAG_B]` — a bitmask combination is expected.
- `valuesFromClass: MyClass::class` — one of the class's constants.
- `flagsFromClass: MyClass::class` — bitmask of the class's constants.

```php
function phpinfo(
    #[ExpectedValues(flags: [INFO_GENERAL, INFO_CREDITS, INFO_CONFIGURATION,
                             INFO_MODULES, INFO_ENVIRONMENT, INFO_VARIABLES,
                             INFO_LICENSE, INFO_ALL])]
    int $flags = INFO_ALL
): bool {}

function pathinfo(
    string $path,
    #[ExpectedValues(flags: [PATHINFO_DIRNAME, PATHINFO_BASENAME,
                             PATHINFO_EXTENSION, PATHINFO_FILENAME])]
    int $flags = PATHINFO_ALL
): string|array {}
```

**Implementation:** During parameter extraction, store the expected
values metadata. When providing completions inside a function call
argument position, check whether the target parameter has expected
values and offer the listed constants at the top of the suggestions
list. Flag-style parameters should also suggest bitwise-OR
combinations.

---

## C7. `class_alias()` support

**Impact: Low-Medium · Complexity: Medium-High**

Resolve `class_alias('OriginalClass', 'AliasName')` so that the alias
name works for completion, go-to-definition, and hover. PHP's
`class_alias()` creates a runtime alias for a class, and many codebases
rely on this for backwards compatibility layers and framework internals
(Laravel's Facade loader uses `class_alias` to register short names).

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
   (e.g. a `_ide_helper.php` or a framework bootstrap file), the alias
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
   `affinity` and before `demote`. This keeps it as a tiebreaker
   within the same affinity bracket rather than overriding the
   namespace-usage signal. Only apply when the affinity score is zero
   or tied.

3. **Pass the current file path** through `ClassCompletionParams` and
   into `ClassItemCtx` so it's available during sort-text construction.

## C10. Deprecation markers on class-name completions from all sources

**Impact: Low · Complexity: Medium**

Same-namespace classes (source tier 2) already carry deprecation info
because `ClassInfo` is available. Classes from `fqn_uri_index`
and stubs (tiers 3-4) don't check for `@deprecated` because the class
may not be fully loaded at completion time.

For classmap entries, a lightweight byte-level scan of the first
docblock in the file (similar to `detect_stub_class_kind`) could detect
`@deprecated` without a full parse. For stubs, the source is already
in memory and could be scanned cheaply. For fqn_uri_index entries, the
deprecation flag could be stored alongside the file path when the class
is first indexed.

This is a small quality-of-life improvement: deprecated classes would
show with a strikethrough in the completion menu across all sources,
not just same-namespace ones.

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
