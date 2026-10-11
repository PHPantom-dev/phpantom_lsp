# PHPantom — CLI Fix Command

The `fix` subcommand applies automated code fixes across a PHP project,
modeled after php-cs-fixer. Each "rule" corresponds to a diagnostic code
and its associated code action.

## Implemented Rules

### `unused_import` — Remove unused imports

Detects unused `use` statements and removes them. Handles simple imports,
group imports (removing individual members), and blank-line collapsing to
keep formatting clean.

---

## Planned Native Rules

These rules use PHPantom's own diagnostics and do not require external
tools.

### FX1. `deprecated` — Replace deprecated symbol usage

**Prerequisite:** The existing `replace_deprecated` code action
(`src/code_actions/replace_deprecated.rs`), already implemented and
shipped as an LSP quickfix.

When a symbol carries a `#[Deprecated(replacement: "...")]` attribute
(the phpstorm-stubs convention, using `%parametersList%`,
`%parameter0%`, `%class%` template placeholders), automatically apply
the replacement. The action already does this mechanically from the
structured attribute, not from free-text message parsing; the
remaining work is wiring it into `fix.rs`'s `NATIVE_RULES` the same
way `unused_import` is wired.

### FX2. `unused_variable` — Remove unused variables

The unused-variable diagnostic already ships
(`src/diagnostics/unused_variables.rs`), so this rule's diagnostic
prerequisite is satisfied. There is no code action yet to remove the
assignment, so this rule needs both a code action and CLI wiring:
remove assignments to variables that are never read, skipping
variables with side effects in the RHS (method calls, function calls).
When the RHS is pure (literal, property access, simple expression),
remove the entire statement.

### FX7. `add_return_type` — Generate `@return` docblocks from function bodies

The return-type inference this needs already exists and is shared by
two other consumers: the docblock-generation completion item (typing
`/**` above a function) and the "Update docblock to match signature"
quickfix's enrichment of an existing `@return` tag
(`enrichment_return_type` in
`src/code_actions/phpstan/fix_return_type/inference.rs`). Neither of
those triggers fires for a function that has no docblock at all, so
this rule still needs a new trigger path (a code action, or a
dedicated fix-CLI pass) that adds a fresh `@return` tag when a
function or method has a native `array` return type (or no return
type at all) and the body contains enough information to infer a
specific element type, e.g. `@return list<Butterfly>`.

This also covers PHPStan's `missingType.iterableValue` on return types
without running PHPStan: a native `array` return with no element type
is exactly the case above. The "Add `@return` type" quickfix for that
identifier (`src/code_actions/phpstan/add_iterable_type.rs`) carries its
own element-type inference from `return` statements; fold it into
`enrichment_return_type` rather than keeping two inference paths, and
let the quickfix and this rule share the result.

This lets teams that want to reach PHPStan level 6 (require return
type declarations) run a single command and get specific, useful
return types across the entire codebase for free, instead of adding
them by hand file by file.

---

## Planned PHPStan Rules

These rules require running PHPStan first to collect diagnostics.
They are gated behind `--with-phpstan`.

### FX3. `phpstan.return.unusedType` — Remove unused type from return union

**Backlog ID:** H10

The underlying logic already ships as the "Remove unused return type"
LSP quickfix (`src/code_actions/phpstan/remove_unused_return_type.rs`,
matching PHPStan identifier `return.unusedType`): it parses the unused
type from PHPStan's message, finds the return type (native or
`@return`), removes the unused member from the union or intersection,
and simplifies a resulting single-member union. What remains for this
rule is exclusively CLI wiring: invoking the quickfix's resolve
function headlessly from `fix.rs` against diagnostics collected via
`--with-phpstan`.

---

## Infrastructure

### Rule selection

Rules are identified by their diagnostic code string:
- Native rules: bare identifiers (e.g. `unused_import`)
- PHPStan rules: prefixed with `phpstan.` (e.g. `phpstan.return.unusedType`)

When no `--rule` flags are provided, every native rule in
`NATIVE_RULES` (`src/fix.rs`) runs.

PHPStan rules only run when `--with-phpstan` is passed. This is an
explicit opt-in because PHPStan adds significant runtime (it must
analyze the entire project first).

### PHPStan integration

The CLI already accepts `--with-phpstan` (`src/main.rs`, `src/fix.rs`),
but today it only relaxes rule validation; no PHPStan process is ever
run from `fix.rs`. The batch-invocation and JSON-parsing pieces this
needs already exist elsewhere and just need to be called from the fix
pipeline:

1. Run PHPStan on all target files (or the entire project if no path
   filter is given) in a single batch invocation. `run_phpstan_workspace`
   in `src/phpstan.rs` already does this (used today by the LSP's
   workspace-wide external-tool diagnostic proxy).
2. Parse the JSON output to collect diagnostics per file.
   `run_phpstan_workspace` already returns a `HashMap<PathBuf,
   Vec<Diagnostic>>` with each `Diagnostic.code` set to the PHPStan
   identifier (e.g. `return.unusedType`).
3. Match diagnostics to registered PHPStan rules by prefixing the
   diagnostic code with `phpstan.` and comparing to the requested
   rule strings.
4. For each matched diagnostic, invoke the corresponding code action's
   resolve function headlessly and apply the resulting edit. This part
   is unwritten: the existing PHPStan quickfixes
   (`src/code_actions/phpstan/`) are wired for the LSP code-action
   protocol, not for headless batch invocation from `fix.rs`.

To maximize efficiency, PHPStan runs once for all files rather than
per-file.

### Dry-run mode

`--dry-run` reports what would change without writing files. Exit code
`2` indicates fixable issues were found. This is useful for CI
pipelines that want to enforce code style without modifying files.

### Idempotency

Running `fix` twice should produce the same result as running it once.
Each rule must be idempotent: if the fix has already been applied, the
rule should detect no issues and make no changes.

### Exit codes

| Code | Meaning |
|------|---------|
| 0    | Success (fixes applied, or nothing to fix) |
| 1    | Error (bad arguments, write failure, etc.) |
| 2    | Dry-run found fixable issues |
