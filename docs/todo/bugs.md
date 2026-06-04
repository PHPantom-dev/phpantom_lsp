# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.


## B16. PDOStatement fetch mode-dependent return types

**Blocked on:** [phpstorm-stubs#1882](https://github.com/JetBrains/phpstorm-stubs/pull/1882)

`PDOStatement::fetch()` and `PDOStatement::fetchAll()` return
different types depending on the fetch mode constant passed as
the first argument. Once the upstream PR is merged and we update
our stubs, the existing conditional return type support should
handle this automatically.

**Tests:** Assertion lines were removed from
`tests/psalm_assertions/method_call.php` (out of scope until
upstream stubs land).


## B21. Named-argument positional-index confusion

**Severity: Medium.** Multiple call sites match arguments to
parameters by `enumerate()` index / `.nth(param_idx)`, ignoring
that named arguments can appear in any order:

- `src/completion/types/conditional.rs:716` — conditional return
  types misresolve for `func($a, paramName: $b)`; a non-matching
  named arg in the positional slot makes the conditional fall
  through to the else branch.
- `src/diagnostics/argument_count.rs:288` — `actual_args` is the
  raw count, so `f(c: 3)` against `function f($a, $b = 0, $c = 0)`
  raises **no** "missing required argument" diagnostic even though
  PHP throws `ArgumentCountError` (false negative).
- `src/completion/variable/rhs_resolution.rs:1877` and
  `src/completion/variable/forward_walk.rs:5219` — pass-by-ref
  type seeding consults the wrong parameter for named args.

**Fix:** Resolve named arguments by parameter name everywhere
call-args are matched to parameters, via one shared mapping
(positional args fill in order, named args fill by name), and
verify each required parameter is actually supplied.


## B22. Constructor-less classes flagged for too many arguments

**Severity: Medium (false positive).** In
`src/completion/call_resolution.rs:677`, a class with no
`__construct` (and none inherited) returns `Some` with an empty
parameter list instead of `None`. The argument-count collector
(`src/diagnostics/argument_count.rs`) then sees `max = 0`, so with
the opt-in extra-arguments check enabled, `new Foo("x")` on a
constructor-less class is flagged "Expected 0 arguments, got 1".
PHP ignores those arguments (no error), and the diagnostic's own
module doc promises this case is skipped.

**Fix:** Return `None` from the callable resolver for
constructor-less classes so the call is suppressed.


## B23. Leading-backslash builtins bypass overload minimums

**Severity: Medium (false positive).** In
`src/diagnostics/argument_count.rs:288`, the overload-minimum
lookup uses the verbatim call expression. `overload_min_args`
lowercases but does not strip a leading `\`, so `\mt_rand()`,
`\rand()`, `\getenv()`, etc. in namespaced code miss the overload
table and are checked against the stub's full required count
("Expected 2 arguments, got 0").

**Fix:** Strip the leading `\` before the overload lookup.


## B24. Byte length used as UTF-16 column in PHPStan-ignore insertion

**Severity: Medium.** In `src/code_actions/phpstan/ignore.rs:295`,
`build_eol_comment` / `build_add_ignore_edit` set the edit column
to `line_text.len()` (byte length), but LSP positions are UTF-16
code units. On any line containing multibyte characters the
`// @phpstan-ignore` comment is inserted at the wrong column or
past the end of the line.

**Fix:** Convert the byte offset to a UTF-16 column (the codebase
already has `offset_to_position` / `len_utf16` helpers in
`util.rs`).


## B25. Generic substitution does not right-align fewer args

**Severity: Medium.** In `src/inheritance.rs:1183`
(`build_substitution_map`) and `:1120`
(`build_trait_substitution_map`), template args are bound by
direct index zip. `@extends Collection<User>` against
`Collection<TKey, TValue>` binds `TKey => User` and leaves
`TValue` to the bound/`mixed` fallback, so inherited element types
resolve wrong. The sibling `build_generic_subs` (around line 1316)
right-aligns correctly.

**Fix:** Right-align fewer-than-params generic arguments to the
trailing template parameters, matching `build_generic_subs` and
PHPStan/Psalm convention.


## B26. Null-safe receiver chain dropped in subject extraction

**Severity: Medium.** In `src/subject_extraction.rs:600`, the
`?->` branch of `extract_call_subject` uses
`extract_simple_variable` instead of recursing like the `->`
branch does. For `$a->b?->c()->` the receiver `$a->b` is reduced
to the bare identifier `b`, producing the subject `b?->c()`, so
the chain fails to resolve. Any property-or-call chain immediately
followed by `?->method()` is affected.

**Fix:** Recurse through the full receiver in the `?->` branch the
same way the `->` branch does.


## B27. `@see` reference spans off by one

**Severity: Medium.** In `src/symbol_map/docblock.rs:1420`, a
qualified-but-not-FQN reference (`@see App\Models\User` or
`@see App\Foo::bar()`) gets a synthetic `\` prepended into a new
string, but subsequent offset math (`file_offset + reference.len()`,
`member_start = file_offset + sep_pos + 2`) uses the lengthened
string while `file_offset` still points at the original text. The
resulting `ClassReference`/`MemberAccess` spans for
go-to-definition, highlight, and rename are off by one byte.

**Fix:** Compute spans from the original (pre-prefix) lengths and
positions.


## B28. Stale autoload symbols after `composer.json` change

**Severity: Medium.** In `src/server.rs:723`
(`did_change_watched_files`, composer-changed path), old vendor
classes are purged from `fqn_uri_index` by URI prefix, but
`autoload_function_index` and `autoload_constant_index` are only
*inserted* into without first removing entries that pointed into
the old vendor tree. Functions/constants removed by a
`composer update` linger in those indexes, so completion and
go-to-definition keep resolving symbols that no longer exist. (The
per-file `deindex_file` path purges these correctly; only the
composer-wide path misses it.)

**Fix:** Purge the old vendor entries from the function/constant
indexes before re-scanning, the same way `fqn_uri_index` is
purged.


## B29. Property declaration reference range off by one

**Severity: Low.** In `src/references/mod.rs:967`, the declaration
range for a property is `name_offset .. name_offset + prop.name.len()`.
`name_offset` points at the `$` sigil but `prop.name` excludes the
`$`, so the range covers `$nam` instead of `$name`. Every other
variable/property range in the codebase uses `+ 1 + name.len()`.

**Fix:** Account for the `$` sigil (`name_offset + 1 + len`, or
include the `$` in the start).


## B30. Conditional `is null` text path treats missing arg as null

**Severity: Low.** In `src/completion/types/conditional.rs:380`,
`resolve_conditional_with_text_args_and_defaults` takes the
null/`then` branch whenever the textual argument is absent for the
subject parameter, while the AST path keys off `arg_expr.is_none()`
with a different rule. The same call can resolve to different
return types depending on whether the inline-text or AST path runs.

**Fix:** Align the text path with the AST path's "argument
explicitly passed as null" rule.


## B31. `@var` annotations collected file-wide

**Severity: Low (false negative).** In
`src/diagnostics/undefined_variables.rs:1210`,
`collect_var_annotations` scans the entire file's text and adds
every annotated name to the `always_defined` set used for every
function/method in the file. A `/** @var X $foo */` in one method
permanently suppresses "Undefined variable '$foo'" everywhere in
the file, even where `$foo` is a genuine typo. The module header
(lines 13-20) also contradicts the implemented "prior write in
source order" rule documented at line 316.

**Fix:** Scope `@var` annotations to the function/method they
appear in, and reconcile the contradictory module documentation.


## B32. Unused-import range matched by `contains(fqn)`

**Severity: Low (wrong squiggle location).** In
`src/diagnostics/unused_imports.rs:392`, `find_use_statement_range`
matches a non-aliased import with `trimmed.contains(fqn)`. When
two imports share a prefix (`use App\Foo;` and `use App\FooBar;`),
removing the unused `App\Foo` matches the `App\FooBar` line first,
so the "Unused import" hint and `DiagnosticTag::Unnecessary`
dimming land on the wrong `use` statement.

**Fix:** Match the import name with word boundaries (exact segment
match), not a substring `contains`.


## B33. `??` over bare `array` injects `mixed`, poisoning downstream

**Severity: Low.** In
`src/completion/variable/rhs_resolution.rs:435`, when the LHS of
`??` resolves to nothing (e.g. array access on a bare `array`),
the code injects `PhpType::mixed()` and unions it with the RHS, so
`$x = $params['key'] ?? 5` yields `mixed|int` and contaminates
later type checks on `$x`. The inline comment flags this as a
band-aid for the real root cause (array access on a bare `array`
returning empty instead of `mixed`).

**Fix:** Make array access on a bare `array` return `mixed`
directly so the `??` handler does not need the workaround.


## B34. `is_transitive_iterable` recurses only through `parent_class`

**Severity: Low.** In
`src/completion/variable/foreach_resolution.rs:108`, the transitive
iterable check inspects direct `interfaces`, `extends_generics`,
and recurses through `parent_class`, but does not recurse into the
entries of `interfaces`. An interface that extends a known
iterable interface two hops away via `interfaces` is not detected,
so `foreach` element types are missed for such collections.

**Fix:** Recurse through `interfaces` entries as well as
`parent_class`.


## B35. `$this->prop` assignment scan ignores `else`/`elseif`

**Severity: Low.** In
`src/completion/variable/rhs_resolution.rs:4067`,
`find_this_property_assignment_in_toplevel`'s `Statement::If` arm
walks only `if_stmt.body.statements()` (the then-branch).
Assignments to `$this->prop` inside `else`/`elseif` blocks before
the cursor are not found, so the narrowing from such an assignment
is missed (the declared property type is still used as fallback).

**Fix:** Walk the `else`/`elseif` bodies too.


## B36. `split_array_access_key` mis-splits nested array access

**Severity: Low.** In
`src/completion/variable/forward_walk.rs:8583`,
`split_array_access_key` finds the first `["` and strips a trailing
`"]`. For a nested key like `$a["x"]["y"]` it yields base `$a` and
key `x"]["y`, so subsequent shape-key narrowing targets a
nonexistent key and the real narrowing is dropped.

**Fix:** Split on the outermost balanced bracket pair (or only
handle single-level keys explicitly).


## B37. Byte-vs-UTF16 column confusion on multibyte lines

**Severity: Low.** Several handlers emit a byte offset where LSP
expects a UTF-16 column, so positions are wrong on lines with
multibyte characters:

- `src/signature_help.rs:682` — `patch_content_for_signature` uses
  a char index for a UTF-16 column and rejoins with `\n`.
- `src/definition/member/file_lookup.rs:200` — virtual-member
  fallback returns `line.find(...)` (byte offset) as the
  `Position.character`.
- `src/completion/named_args.rs:426` — `split_args_top_level`
  mixes byte length with char indexing in its escape-counting,
  mis-terminating strings that contain multibyte chars before an
  escaped quote.
- `src/code_actions/remove_unused_import.rs:427` — group-member
  removal slices the line by UTF-16 diagnostic columns as byte
  indices (potential non-char-boundary panic).
- `src/php_type.rs:3464,3551` — `replace_star_wildcards` and
  `strip_variance_annotations_from_type` rebuild strings with
  `bytes[i] as char`, mangling multibyte characters in type
  strings that contain a `*` wildcard or a variance annotation.

**Fix:** Convert byte offsets to UTF-16 columns (and iterate by
`char`/grapheme, not raw bytes) in these paths, using the shared
`util.rs` helpers.


## B38. Document symbols: non-class range equals name-only range

**Severity: Low.** In `src/document_symbols.rs:217` (and the
method/property/constant/function arms), the full `range` is set
equal to the name-only `selection_range`. The LSP spec requires
`range` to enclose the whole declaration with `selection_range`
nested inside it; editors that rely on the full range for
breadcrumbs/folding extent are affected. (Class symbols correctly
use keyword→end.)

**Fix:** Compute the full declaration range (signature/body) for
non-class symbols.


## B39. Type hierarchy: class-name scan skips only spaces/tabs

**Severity: Low.** In `src/type_hierarchy.rs:286` (`find_name_start`),
the scan after the class keyword skips only `' '` and `'\t'`, so
the legal-but-rare `class\nFoo {` layout stops at the newline and
mislocates the selection range. `document_symbols.rs`'s
`find_name_after_keyword` skips all whitespace, so the two diverge.

**Fix:** Skip all whitespace (including newlines) when locating the
class name.


## B40. Composer classmap single-quote unescape order

**Severity: Low.** In `src/composer.rs:230`,
`parse_autoload_classmap` unescapes with
`.replace("\\\\'", "'").replace("\\\\", "\\")`. PHP single-quoted
strings escape a literal quote as `\'` (two chars), not `\\'`
(three chars), so the first replacement matches the wrong
sequence. Harmless in practice (class names never contain `'`) but
the logic is wrong.

**Fix:** Unescape `\\` and `\'` per PHP single-quote rules in the
correct order.


## B41. `value-of` over array shape dedups only adjacent values

**Severity: Low (cosmetic).** In `src/php_type.rs:4213`,
`evaluate_value_of` for an `ArrayShape` calls `Vec::dedup()`, which
removes only consecutive duplicates, so `array{a: int, b: string,
c: int}` yields `int|string|int` instead of `int|string`. The
redundant union is not incorrect, just wider than necessary.

**Fix:** Deduplicate the full value set (sort/dedup or a set)
before building the union.

