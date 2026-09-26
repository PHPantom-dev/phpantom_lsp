# PHPantom — Roadmap

This document tracks planned work for PHPantom. Each item links to a
domain document with full context. Items are grouped into time-boxed
sprints (roughly 1-2 weeks each) and a backlog of ideas not yet
scheduled.

**Guiding priorities:** Completion accuracy → Type intelligence →
Cross-file navigation → Diagnostics → Code actions → Performance.

Items inside each sprint are ordered by priority (top = do first):
low-complexity items (easiest to assign, least implementation risk)
before heavy lifts, dependencies before their dependents, and within
the same complexity tier by impact descending. The backlog is ordered
by impact (descending), then complexity (ascending) within the same
impact tier.

**Complexity measures how hard the task is to implement correctly,
not how long it takes** (time estimates are consistently wrong; skill
required is not). Use it to decide who to assign: a **Low**-complexity
item (e.g. adding a hundred repetitive docblock tags) is safe for a
low-skill contributor even if it's tedious; a **High**-complexity item
(e.g. a 50-line change to the forward walker) needs an experienced
contributor even though it's short.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

# Scheduled Sprints

## Sprint 7 — 0.11.0 Blade support

| #    | Item                                                                                                                                                      | Impact      | Complexity  |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
|     | Clear [refactoring gate](todo/refactor.md)                                                                                                                      | —           | —           |
| P63  | [Every diagnostic converts its offsets by counting from the top of the file](todo/performance.md#p63-every-diagnostic-converts-its-offsets-by-counting-from-the-top-of-the-file) | High | Low |
| C14  | [`examples/php` has no demo for an assignment inside a `match` arm or ternary branch](todo/completion.md#c14-examplesphp-has-no-demo-for-an-assignment-inside-a-match-arm-or-ternary-branch) | Low | Low |
| A47  | [Member actions are missing when the range starts in the indentation](todo/actions.md#a47-member-actions-are-missing-when-the-range-starts-in-the-indentation) | Medium | Low-Medium |
| P64  | [A file with one very large scope copies it at every branch](todo/performance.md#p64-a-file-with-one-very-large-scope-copies-it-at-every-branch) | Medium | Medium-High |
| BL1  | [Blade-aware code actions](todo/blade.md#bl1-blade-aware-code-actions)                                                      | Medium     | Medium-High |
|      | **Release 0.11.0**                                                                                                                                        |             |             |

## Sprint 8 — 1.0 release & IDE extensions

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
|     | Clear [refactoring gate](todo/refactor.md)                                                                                                                      | —           | —           |
| X17  | [Index the workspace's other folders](todo/indexing.md#x17-index-the-workspaces-other-folders)                              | Medium      | Medium      |
| X14  | [Ask Zed to expose `file_scan_exclusions` and `file_types` to extensions](todo/indexing.md#x14-ask-zed-to-expose-file_scan_exclusions-and-file_types-to-extensions) (upstream request) | Low | Low |
| E1  | [External stub packages (ide-helper, etc.)](todo/external-stubs.md#e1-project-level-phpstorm-stubs-for-gtd)                                                     | Medium-High | Low         |
| E5  | [Extension stub coverage audit](todo/external-stubs.md#e5-extension-stub-selection-stubs-extensions)                                                            | Medium      | Low         |
| E4  | [Embedded stub override with external stubs](todo/external-stubs.md#e4-embedded-stub-override-with-external-stubs) (depends on E1)                              | Medium      | Low         |
| E3  | [IDE-provided and `.phpantom.toml` stub paths](todo/external-stubs.md#e3-ide-provided-and-phpantomtoml-stub-paths) (depends on E2)                              | Low-Medium  | Low         |
| D10 | [PHPMD diagnostic proxy](todo/diagnostics.md#d10-phpmd-diagnostic-proxy)                                              | Low        | Medium |
| D25  | [Two traits declaring the same property with different types is not reported](todo/diagnostics.md#d25-two-traits-declaring-the-same-property-with-different-types-is-not-reported) | Low | Low-Medium |
| D24  | [A `match` that does not cover every enum case is not reported](todo/diagnostics.md#d24-a-match-that-does-not-cover-every-enum-case-is-not-reported) | Medium | Medium |
| D27  | [Destructuring offsets an array cannot have is not reported](todo/diagnostics.md#d27-destructuring-offsets-an-array-cannot-have-is-not-reported) | Low | Medium |
| T13  | [Closure variables lose callable signature detail](todo/type-inference.md#t13-closure-variables-lose-callable-signature-detail) | Low-Medium | Medium-High |
| D26  | [Reading a typed property that nothing initialises is not reported](todo/diagnostics.md#d26-reading-a-typed-property-that-nothing-initialises-is-not-reported) | Low-Medium | Medium-High |
| L1  | [Facade completion](todo/laravel.md#l1-facade-completion-upstream-method-generator-improvement) (upstream `facade-documenter` PRs)                              | High        | High        |
| E2  | [Project-level stubs as type resolution source](todo/external-stubs.md#e2-project-level-stubs-as-resolution-source) (depends on E1)                             | Medium      | High        |
| F20 | [Migrate to the maintained `tower-lsp` fork](todo/lsp-features.md#f20-migrate-to-the-maintained-tower-lsp-fork)                                              | Low-Medium  | Very High   |
| F21 | [Static `typeHierarchyProvider` advertisement](todo/lsp-features.md#f21-static-typehierarchyprovider-advertisement-depends-on-f20) (depends on F20; also needs an upstream `lsp-types` fix) | Low-Medium  | Low         |
|     | **Release 1.0.0 + IDE extensions**                                                                                                                              |             |             |

# Backlog

Items not yet assigned to a sprint. Worth doing eventually but
unlikely to move the needle for most users.

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
|     | **[Completion](todo/completion.md)**                                                                                                                                        |             |             |
| C1  | Array functions needing new code paths                                                                                                                                      | Medium      | High        |
| C11 | [Smarter member ordering after `->` / `::`](todo/completion.md#c11-smarter-member-ordering-after-)                                                                       | Medium      | High        |
| C8  | [Filesystem proximity as an affinity tiebreaker](todo/completion.md#c8-filesystem-proximity-as-an-affinity-tiebreaker)                                                      | Low-Medium  | Medium      |
| C3  | Go-to-definition for array shape keys via bracket access                                                                                                                    | Low-Medium  | Medium      |
| C7  | `class_alias()` support                                                                                                                                                     | Low-Medium  | Medium-High |
| C5  | `#[ReturnTypeContract]` parameter-dependent return types                                                                                                                    | Low         | Medium      |
| C6  | `#[ExpectedValues]` parameter value suggestions                                                                                                                             | Low         | Medium      |
| C10 | [Deprecation markers on class-name completions from all sources](todo/completion.md#c10-deprecation-markers-on-class-name-completions-from-all-sources)                     | Low         | Medium      |
| C12 | [The implicit `$value` of a `set` hook is not offered by variable completion](todo/completion.md#c12-the-implicit-value-of-a-set-hook-is-not-offered-by-variable-completion) | Low         | Medium      |
| C13 | [`self::`/`static::` inside a `@require-extends` trait does not see the required class's static members](todo/completion.md#c13-selfstatic-inside-a-require-extends-trait-does-not-see-the-required-classs-static-members) | Low-Medium  | Medium      |
| C4  | Non-array functions with dynamic return types                                                                                                                               | Low         | High        |
|     | **[Type Inference](todo/type-inference.md)**                                                                                                                                |             |             |
| T20 | [Type narrowing reconciliation engine](todo/type-inference.md#t20-type-narrowing-reconciliation-engine) (CNF clause algebra, sure/sureNot tracking)                         | Medium-High | Very High   |
| T41 | [`@param-out` is parsed but never read](todo/type-inference.md#t41-param-out-is-parsed-but-never-read)                                                                      | Medium      | Medium      |
| T28 | [Template inference depth priority (shallowest bound wins)](todo/type-inference.md#t28-template-inference-depth-priority-shallowest-bound-wins)                             | Medium      | Medium-High |
| T3  | [Property hooks (PHP 8.4)](todo/type-inference.md#t3-property-hooks-php-84)                                                                                                 | Medium      | Medium-High |
| T42 | [A static factory's own `@return self<T>` breaks every chained call after it](todo/type-inference.md#t42-a-static-factorys-own-return-selft-breaks-every-chained-call-after-it) | Medium      | Unknown     |
| T43 | [`self::TypeAlias` inside `@extends`'s generic argument is not resolved](todo/type-inference.md#t43-selftypealias-inside-extendss-generic-argument-is-not-resolved) | Low         | Medium      |
| T34 | [`static::CONST` over-narrows to the declaring class's value](todo/type-inference.md#t34-staticconst-over-narrows-to-the-declaring-classs-value)                            | Medium      | Medium-High |
| T29 | [Definite vs possible variable existence tracking](todo/type-inference.md#t29-definite-vs-possible-variable-existence-tracking)                                             | Medium      | High        |
| T30 | [Literal type collapse limit](todo/type-inference.md#t30-literal-type-collapse-limit)                                                                                       | Low-Medium  | Medium      |
| T40 | [`pathinfo()` returns a shape or a string depending on the flags argument](todo/type-inference.md#t40-pathinfo-returns-a-shape-or-a-string-depending-on-the-flags-argument) | Low-Medium  | Medium      |
| T26 | [Class constants named as docblock types (`Foo::BAR`, `Foo::BAR_*`)](todo/type-inference.md#t26-class-constants-named-as-docblock-types-foobar-foobar_) | Low-Medium  | Medium      |
| T33 | [Class constant on an expression (`$obj::CONST`) resolves to nothing](todo/type-inference.md#t33-class-constant-on-an-expression-objconst-resolves-to-nothing)              | Low-Medium  | Medium      |
| T6  | `Closure::bind()` / `Closure::fromCallable()` return type preservation                                                                                                      | Low-Medium  | Medium-High |
| T31 | [Closure literal-return shape inference](todo/type-inference.md#t31-closure-literal-return-shape-inference)                                                                 | Low-Medium  | Medium-High |
| T44 | [A single enum case has no type](todo/type-inference.md#t44-a-single-enum-case-has-no-type) | Low-Medium | High |
| T4  | [Non-empty-\* type narrowing and propagation](todo/type-inference.md#t4-non-empty--type-narrowing-and-propagation)                                                          | Low-Medium  | High        |
| T5  | Fiber type resolution                                                                                                                                                       | Low         | Medium      |
| T10 | [Ternary expression as RHS of list destructuring](todo/type-inference.md#t10-ternary-expression-as-rhs-of-list-destructuring)                                               | Low         | Medium      |
| T11 | [Nested list destructuring](todo/type-inference.md#t11-nested-list-destructuring)                                                                                           | Low         | Medium      |
|     | **[Bugs](todo/bugs.md)**                                                                                                                                                    |             |             |
| B457 | [`array_push()` does not change the array's type](todo/bugs.md#b457-array_push-does-not-change-the-arrays-type) | Medium | Low-Medium |
| B437 | [`self` in an inherited property's docblock names the class it is read through](todo/bugs.md#b437-self-in-an-inherited-propertys-docblock-names-the-class-it-is-read-through) | Medium | Medium |
| B438 | [An inline `@phpstan-var` above an assignment is ignored](todo/bugs.md#b438-an-inline-phpstan-var-above-an-assignment-is-ignored) | Medium | Medium |
| B442 | [`$value::class` resolves to nothing](todo/bugs.md#b442-valueclass-resolves-to-nothing) | Low-Medium | Low |
| B439 | [`@mixin` on a trait is not applied to the class that uses it](todo/bugs.md#b439-mixin-on-a-trait-is-not-applied-to-the-class-that-uses-it) | Low-Medium | Medium |
| B446 | [Writing into a property's array offset does not narrow the property](todo/bugs.md#b446-writing-into-a-propertys-array-offset-does-not-narrow-the-property) | Low-Medium | Medium |
| B456 | [Writes through a list's own keys drop `list`, while `unset()` of an element keeps it](todo/bugs.md#b456-writes-through-a-lists-own-keys-drop-list-while-unset-of-an-element-keeps-it) | Low-Medium | Medium |
| B459 | [Writing a literal into an array offset widens it to its base type](todo/bugs.md#b459-writing-a-literal-into-an-array-offset-widens-it-to-its-base-type) | Low-Medium | Medium |
| B465 | [A new object with unbound templates assigned to a generic property keeps the bounds](todo/bugs.md#b465-a-new-object-with-unbound-templates-assigned-to-a-generic-property-keeps-the-bounds) | Low-Medium | Medium |
| B458 | [An `(array)` cast of a union with a non-array member is plain `array`](todo/bugs.md#b458-an-array-cast-of-a-union-with-a-non-array-member-is-plain-array) | Low | Low |
| B476 | [Calling a closure declared `: static` gives a bare `static`](todo/bugs.md#b476-calling-a-closure-declared--static-gives-a-bare-static) | Low | Low |
| B474 | [A magic constant resolves to its base type rather than its value](todo/bugs.md#b474-a-magic-constant-resolves-to-its-base-type-rather-than-its-value) | Low | Low-Medium |
| B479 | [`??=` on an offset whose key is not a variable leaves the offset `null`](todo/bugs.md#b479--on-an-offset-whose-key-is-not-a-variable-leaves-the-offset-null) | Low | Low-Medium |
| B440 | [`extract()` defines no variables](todo/bugs.md#b440-extract-defines-no-variables) | Low | Medium |
| B441 | [A class constant whose initializer names an enum case is not resolved](todo/bugs.md#b441-a-class-constant-whose-initializer-names-an-enum-case-is-not-resolved) | Low | Medium |
| B443 | [A closure parameter is not inferred from a `callable(static)` type alias declared on a trait](todo/bugs.md#b443-a-closure-parameter-is-not-inferred-from-a-callablestatic-type-alias-declared-on-a-trait) | Low | Medium |
| B447 | [`array_key_exists()` with a variable key does not narrow the offset read](todo/bugs.md#b447-array_key_exists-with-a-variable-key-does-not-narrow-the-offset-read) | Low | Medium |
| B449 | [Exhausted narrowing leaves the last type standing instead of `never`](todo/bugs.md#b449-exhausted-narrowing-leaves-the-last-type-standing-instead-of-never) | Low | Medium |
| B460 | [Array shape unions are merged differently from PHPStan](todo/bugs.md#b460-array-shape-unions-are-merged-differently-from-phpstan) | Low | Medium |
| B463 | [A method template with a bound is shown as its bound inside the method](todo/bugs.md#b463-a-method-template-with-a-bound-is-shown-as-its-bound-inside-the-method) | Low | Medium |
| B464 | [Template inference from a literal argument widens it](todo/bugs.md#b464-template-inference-from-a-literal-argument-widens-it) | Low | Medium |
| B466 | [A template nested in `class-string<Foo<T>>` is not inferred](todo/bugs.md#b466-a-template-nested-in-class-stringfoot-is-not-inferred) | Low | Medium |
| B469 | [A conditional return type whose subject is an offset of a template is not evaluated](todo/bugs.md#b469-a-conditional-return-type-whose-subject-is-an-offset-of-a-template-is-not-evaluated) | Low | Medium |
| B470 | [An offset access on a type alias is not evaluated](todo/bugs.md#b470-an-offset-access-on-a-type-alias-is-not-evaluated) | Low | Medium |
| B480 | [An argument outside a method template's bound binds the template anyway](todo/bugs.md#b480-an-argument-outside-a-method-templates-bound-binds-the-template-anyway) | Low | Medium |
| B472 | [A template bound through a nested callable parameter is not inferred](todo/bugs.md#b472-a-template-bound-through-a-nested-callable-parameter-is-not-inferred) | Low | Medium |
| B475 | [A variable a closure captures by reference does not take on the closure's assignments](todo/bugs.md#b475-a-variable-a-closure-captures-by-reference-does-not-take-on-the-closures-assignments) | Low | Medium |
| B477 | [An arrow function's return type comes from its declared hint even when the body returns something narrower](todo/bugs.md#b477-an-arrow-functions-return-type-comes-from-its-declared-hint-even-when-the-body-returns-something-narrower) | Low | Medium |
| B468 | [A conditional return type on `$param is not null` does not pick up a template bound by a callable argument](todo/bugs.md#b468-a-conditional-return-type-on-param-is-not-null-does-not-pick-up-a-template-bound-by-a-callable-argument) | Low | Medium-High |
|     | **[Diagnostics](todo/diagnostics.md)**                                                                                                                                      |             |             |
| D6  | [Unreachable code diagnostic](todo/diagnostics.md#d6-unreachable-code-diagnostic)                                                                                           | Low-Medium  | Medium      |
| D16 | [`unreachable_match_arm` ignores literal subject types](todo/diagnostics.md#d16-unreachable_match_arm-ignores-literal-subject-types)                                        | Low-Medium  | Medium      |
| D5  | [External tool diagnostic suppression actions](todo/diagnostics.md#d5-external-tool-diagnostic-suppression-actions)                                                         | Low         | Low         |
| D15 | [Unused parameter diagnostic](todo/diagnostics.md#d15-unused-parameter-diagnostic)                                                                                          | Low         | Medium      |
| D17 | [`docblock_native_mismatch` only judges nullability](todo/diagnostics.md#d17-docblock_native_mismatch-only-judges-nullability)                                              | Low         | Medium-High |
| D18 | [`array<int, T>` is accepted wherever a `list<T>` is declared](todo/diagnostics.md#d18-arrayint-t-is-accepted-wherever-a-listt-is-declared)                                 | Low         | Medium-High |
| D19 | [`invalid_member_access` cannot tell a property read from a write](todo/diagnostics.md#d19-invalid_member_access-cannot-tell-a-property-read-from-a-write)                    | Low-Medium  | Medium      |
| D20 | [`Foo::$bar` and `Foo::bar` are the same span](todo/diagnostics.md#d20-foobar-and-foobar-are-the-same-span)                                                                  | Low         | Medium      |
| D21 | [A union of an unreachable and a missing member is reported by neither check](todo/diagnostics.md#d21-a-union-of-an-unreachable-and-a-missing-member-is-reported-by-neither-check) | Low         | Medium-High |
| D22 | [Member provenance is recomputed instead of recorded](todo/diagnostics.md#d22-member-provenance-is-recomputed-instead-of-recorded)                                        | Medium      | Medium-High |
| D23 | [A rebound closure's scope is added to the lexical one rather than replacing it](todo/diagnostics.md#d23-a-rebound-closures-scope-is-added-to-the-lexical-one-rather-than-replacing-it) | Low-Medium  | Medium      |
|     | **[Code Actions](todo/actions.md)**                                                                                                                                         |             |             |
| A40 | [Generate method from call](todo/actions.md#a40-generate-method-from-call)                                                                                                  | Medium-High | Medium-High |
| A28 | [Explicit nullable parameter type](todo/actions.md#a28-explicit-nullable-parameter-type-php-84-deprecation) (PHP 8.4 deprecation)                                           | Medium      | Low         |
| A16 | [Snippet placeholder for extracted method name](todo/actions.md#a16-snippet-placeholder-for-extracted-method-name) (lets the user type over the generated name immediately) | Medium      | Medium      |
| A46 | [Honor `context.only` in code action responses](todo/actions.md#a46-honor-contextonly-in-code-action-responses)                                                             | Medium      | Medium      |
| A25 | [`strpos` → `str_contains`](todo/actions.md#a25-strpos-str_contains-php-80) (PHP 8.0+)                                                                                     | Medium      | Medium      |
| A41 | [Create class from non-existing name](todo/actions.md#a41-create-class-from-non-existing-name)                                                                              | Medium      | Medium-High |
| A34 | [Unified code action handler architecture](todo/actions.md#a34-unified-code-action-handler-architecture) (closure-based resolve, unified fix type)                          | Medium      | Very High   |
| A29 | [Simplify boolean return](todo/actions.md#a29-simplify-boolean-return) (`if (cond) return true; return false;` → `return cond;`)                                            | Low-Medium  | Medium      |
| A45 | [Simplify with `?:`](todo/actions.md#a45-simplify-with-elvis-operator) (replace `$x ? $x : $y` with `$x ?: $y`)                                                             | Low-Medium  | Medium      |
| A31 | [Remove always-else](todo/actions.md#a31-remove-always-else-extract-guard-clause) (extract guard clause)                                                                    | Low-Medium  | Medium-High |
| A37 | [Simplify with `?->`](todo/actions.md#a37-simplify-with-nullsafe-operator) (replace null-checked chains with the nullsafe operator)                                       | Low-Medium  | Medium-High |
| A38 | [Convert if/elseif chain to switch](todo/actions.md#a38-convert-ifelseif-chain-to-switch)                                                                                   | Low-Medium  | Medium-High |
| A43 | [Update docblock generics](todo/actions.md#a43-update-docblock-generics)                                                                                                    | Low         | Medium      |
|     | **[PHPStan Code Actions](todo/phpstan-actions.md)**                                                                                                                         |             |             |
| H4  | `assign.byRefForeachExpr` — unset by-reference foreach variable                                                                                                             | Medium      | Medium      |
| H13 | `property.notFound` — declare missing property (same-class)                                                                                                                 | Medium      | Medium      |
| H15 | Template bound from tip — add `@template T of X`                                                                                                                            | Medium      | Medium      |
| H16 | `match.unhandled` — add missing match arms                                                                                                                                  | Medium      | Medium-High |
| H19 | `property.unused` / `method.unused` — remove unused member                                                                                                                  | Low         | Low         |
| H23 | `instanceof.alwaysTrue` — remove redundant instanceof check                                                                                                                 | Low         | Low         |
| H24 | `catch.neverThrown` — remove unnecessary catch clause                                                                                                                       | Low         | Low         |
| H20 | `generics.callSiteVarianceRedundant` — remove redundant variance annotation                                                                                                 | Low         | Medium      |
|     | **[CLI Fix Rules](todo/fix-cli.md)**                                                                                                                                        |             |             |
| FX7 | [`add_return_type` — generate `@return` docblocks from function bodies](todo/fix-cli.md#fx7-add_return_type-generate-return-docblocks-from-function-bodies)                | Medium-High | Medium-High |
| FX1 | [`deprecated` — replace deprecated symbol usage](todo/fix-cli.md#fx1-deprecated-replace-deprecated-symbol-usage)                                                           | Medium      | Low         |
| FX3 | [`phpstan.return.unusedType` — remove unused type from return union](todo/fix-cli.md#fx3-phpstanreturnunusedtype-remove-unused-type-from-return-union)                     | Medium      | Low         |
| FX4 | [`phpstan.missingType.iterableValue` — add `@return` with iterable type](todo/fix-cli.md#fx4-phpstanmissingtypeiterablevalue-add-return-with-iterable-type)                | Medium      | Low         |
| FX2 | [`unused_variable` — remove unused variables](todo/fix-cli.md#fx2-unused_variable-remove-unused-variables)                                                                 | Medium      | Medium      |
| FX5 | [`phpstan.property.unused` / `phpstan.method.unused` — remove unused member](todo/fix-cli.md#fx5-phpstanpropertyunused-phpstanmethodunused-remove-unused-member)          | Low         | Low         |
| FX6 | [`phpstan.generics.callSiteVarianceRedundant` — remove redundant variance](todo/fix-cli.md#fx6-phpstangenericscallsitevarianceredundant-remove-redundant-variance)         | Low         | Medium      |
|     | **[LSP Features](todo/lsp-features.md)**                                                                                                                                    |             |             |
| F11 | [VS Code extension](todo/lsp-features.md#f11-vs-code-extension)                                                                                                              | High        | Medium-High |
| F12 | [IntelliJ / PHPStorm plugin](todo/lsp-features.md#f12-intellij-phpstorm-plugin)                                                                                            | High        | Medium-High |
| F13 | [Homebrew formula](todo/lsp-features.md#f13-homebrew-formula)                                                                                                                | Medium      | Low         |
| F17 | [Wire class move to `workspace/willRenameFiles`](todo/lsp-features.md#f17-wire-class-move-to-workspacewillrenamefiles)                                                       | Medium      | Medium      |
| F23 | [Rename a class through its YAML/XML occurrences](todo/lsp-features.md#f23-rename-a-class-through-its-yamlxml-occurrences)                                                  | Medium      | Medium      |
| F2  | [Partial result streaming via `$/progress`](todo/lsp-features.md#f2-partial-result-streaming-via-progress)                                                                  | Medium      | Medium-High |
| F7  | [Evaluatable expression support (DAP integration)](todo/lsp-features.md#f7-evaluatable-expression-support-dap-integration)                                                  | Low-Medium  | Low         |
| F15 | [Go-to-declaration](todo/lsp-features.md#f15-go-to-declaration)                                                                                                              | Low-Medium  | Low         |
| F14 | [Helix upstream PR](todo/lsp-features.md#f14-helix-upstream-pr) (depends on F13)                                                                                            | Low-Medium  | Low         |
| F22 | [Merge a namespace onto one that shares a class name](todo/lsp-features.md#f22-merge-a-namespace-onto-one-that-shares-a-class-name)                                        | Low-Medium  | Medium-High |
| F16 | [On-type `}` brace de-indent](todo/lsp-features.md#f16-on-type-brace-de-indent)                                                                                            | Low         | Low         |
| F19 | [Connect to a remote/TCP language server](todo/lsp-features.md#f19-connect-to-a-remotetcp-language-server-vs-code-extension)                                               | Low         | Medium      |
|     | **[Signature Help](todo/signature-help.md)**                                                                                                                                |             |             |
| S2  | [Closure / arrow function parameter signature help](todo/signature-help.md#s2-closure-arrow-function-parameter-signature-help)                                             | Medium      | Medium      |
| S3  | Multiple overloaded signatures                                                                                                                                              | Medium      | Medium-High |
| S4  | Named argument awareness in active parameter                                                                                                                                | Low-Medium  | Medium      |
| S5  | Language construct signature help and hover                                                                                                                                 | Low         | Medium      |
|     | **[Laravel](todo/laravel.md)**                                                                                                                                              |             |             |
| L24 | [Translation depth: JSON lang files, locales, placeholders](todo/laravel.md#l24-translation-depth-json-lang-files-locales-placeholders)                                     | Medium-High | Medium-High |
| L46 | [`->can()` on a user model the receiver does not name](todo/laravel.md#l46-can-on-a-user-model-the-receiver-does-not-name)                                                  | Medium-High | Medium-High |
| L30 | [Eloquent attribute-array key completion](todo/laravel.md#l30-eloquent-attribute-array-key-completion)                                                                      | Medium      | Medium      |
| L53 | [Collection key types from the column for `keyBy` / `groupBy` / `pluck`](todo/laravel.md#l53-collection-key-types-from-the-column-for-keyby-groupby-pluck)                   | Medium      | Medium      |
| L32 | [Config-backed named-resource strings](todo/laravel.md#l32-config-backed-named-resource-strings) (log channels, cache stores, guards, connections, rate limiters)           | Medium      | Medium      |
| L49 | [Unguarded Eloquent mass assignment diagnostic](todo/laravel.md#l49-unguarded-eloquent-mass-assignment-diagnostic)                                                          | Medium      | Medium      |
| L17 | [Additional string contexts without booting](todo/laravel.md#l17-additional-string-contexts-without-booting) (middleware, assets, validation, Inertia)                     | Medium      | Medium-High |
| L54 | [Audit custom-builder and relation-closure inference against the PHPStan extensions](todo/laravel.md#l54-audit-custom-builder-and-relation-closure-inference-against-the-phpstan-extensions) | Medium      | Medium-High |
| L31 | [String-key rename, highlight, and semantic tokens](todo/laravel.md#l31-string-key-rename-highlight-and-semantic-tokens)                                                    | Low-Medium  | Medium      |
| L42 | [Morph alias completion in array positions](todo/laravel.md#l42-morph-alias-completion-in-array-positions)                                                                  | Low-Medium  | Medium      |
| L3  | `$dates` array (deprecated)                                                                                                                  | Low-Medium  | Medium      |
| L44 | [Sibling resource registrations and degenerate resource names](todo/laravel.md#l44-sibling-resource-registrations-and-degenerate-resource-names)                             | Low-Medium  | Medium      |
| L50 | ["Create route" quick-fix for an unresolved route name](todo/laravel.md#l50-create-route-quick-fix-for-an-unresolved-route-name)                                            | Low-Medium  | Medium      |
| L47 | [Morph aliases in `*_type` column comparisons](todo/laravel.md#l47-morph-aliases-in-_type-column-comparisons)                                                               | Low-Medium  | Medium-High |
| L8  | `withSum`/`withAvg`/`withMin`/`withMax` aggregate properties                                                                                                                | Low-Medium  | High        |
| L45 | [`*_count` properties are offered on every relationship](todo/laravel.md#l45-_count-properties-are-offered-on-every-relationship)                                           | Low-Medium  | High        |
| L29 | [Livewire and Volt component names](todo/laravel.md#l29-livewire-and-volt-component-names) (Livewire projects only)                                                          | Low         | Low         |
| L27 | [Legacy `Controller@method` action strings](todo/laravel.md#l27-legacy-controllermethod-action-strings)                                                                     | Low         | Low         |
| L10 | `View::withX()` / `RedirectResponse::withX()` dynamic methods                                                                                                               | Low         | Medium      |
| L39 | [Unused view and translation key detection](todo/laravel.md#l39-unused-view-and-translation-key-detection)                                                                  | Low         | Medium      |
| L51 | ["Convert facade call to dependency injection" refactor](todo/laravel.md#l51-convert-facade-call-to-dependency-injection-refactor)                                          | Low         | Medium      |
|     | **[External Stubs](todo/external-stubs.md)**                                                                                                                                |             |             |
| E7  | [Stub-based framework patches](todo/external-stubs.md#e7-stub-based-framework-patches)                                                                                      | Medium      | Medium-High |
| E6  | Stub install prompt for non-Composer projects                                                                                                                               | Low         | Medium      |
|     | **[Performance](todo/performance.md)**                                                                                       |             |             |
| P16 | [Pre-parsed stub format (eliminate raw PHP embedding)](todo/performance.md#p16-pre-parsed-stub-format-eliminate-raw-php-embedding)                                          | High        | Very High   |
| P35 | [Diagnostic passes reach only a fraction of available cores](todo/performance.md#p35-diagnostic-passes-reach-only-a-fraction-of-available-cores)                            | Medium-High | Very High   |
| P30 | [Evaluate migrating parse/resolve/docblock pipeline to `mago-hir`](todo/performance.md#p30-evaluate-migrating-parseresolvedocblock-pipeline-to-mago-hir) (parked — re-evaluated at mago 1.46.0, still no `mago-hir` consumers upstream) | Medium-High | Very High   |
| P52 | [The diagnostic benchmarks measure a path no consumer takes](todo/performance.md#p52-the-diagnostic-benchmarks-measure-a-path-no-consumer-takes)                            | Medium      | Low         |
| P53 | [The deprecated collector deep-copies a class per member access](todo/performance.md#p53-the-deprecated-collector-deep-copies-a-class-per-member-access)                    | Medium      | Low         |
| P51 | [CI-gated scaling and memory invariants](todo/performance.md#p51-ci-gated-scaling-and-memory-invariants)                                                                    | Medium      | Low-Medium  |
| P57 | [Narrowing deep-copies a class every time it crosses the `Arc` boundary](todo/performance.md#p57-narrowing-deep-copies-a-class-every-time-it-crosses-the-arc-boundary)      | Medium      | Medium-High |
| P17 | [`mago-names` resolution on the parse hot path](todo/performance.md#p17-mago-names-resolution-on-the-parse-hot-path)                                                        | Medium      | High        |
| P18 | [Subtype result caching](todo/performance.md#p18-subtype-result-caching) (per-request HashMap for hierarchy walks)                                                          | Medium      | High        |
| P47 | [The resolved-class cache lock caps concurrent class resolution](todo/performance.md#p47-the-resolved-class-cache-lock-caps-concurrent-class-resolution)                     | Medium      | Very High   |
| P20 | [Content-hash gated resolution cache persistence](todo/performance.md#p20-content-hash-gated-resolution-cache-persistence)                                                  | Medium      | Very High   |
| P21 | [Offset-shifting for cached diagnostics on partial edits](todo/performance.md#p21-offset-shifting-for-cached-diagnostics-on-partial-edits)                                  | Medium      | Very High   |
| P3  | Parallel pre-filter in `find_implementors`                                                                                                                                  | Low-Medium  | Medium-High |
| P50 | [Cache the top-level scope for `global` keyword resolution](todo/performance.md#p50-cache-the-top-level-scope-for-global-keyword-resolution)                                 | Low-Medium  | High        |
| P65 | [Every call site repeats the full function lookup, hit or miss](todo/performance.md#p65-every-call-site-repeats-the-full-function-lookup-hit-or-miss) | Low-Medium  | Medium      |
| P58 | [A member-completion cache hit copies the whole item list](todo/performance.md#p58-a-member-completion-cache-hit-copies-the-whole-item-list)                                | Low         | Low         |
| P66 | [Stub version filtering rescans a stub file once per symbol it declares](todo/performance.md#p66-stub-version-filtering-rescans-a-stub-file-once-per-symbol-it-declares) | Low         | Low-Medium  |
| P48 | [Higher-order collection proxy injection repeats work](todo/performance.md#p48-higher-order-collection-proxy-injection-repeats-work)                                        | Low         | Medium      |
| P49 | [A very long method chain costs superlinear time to analyse](todo/performance.md#p49-a-very-long-method-chain-costs-superlinear-time-to-analyse)                              | Low         | Medium      |
| P54 | [Property narrowing re-walks the whole body once per subject](todo/performance.md#p54-property-narrowing-re-walks-the-whole-body-once-per-subject)                          | Low         | Medium      |
| P56 | [Folding array shapes across branches costs superlinear time](todo/performance.md#p56-folding-array-shapes-across-branches-costs-superlinear-time)                            | Low         | Medium      |
| P15 | [Two-phase stub index construction (eliminate `RwLock` on stub maps)](todo/performance.md#p15-two-phase-stub-index-construction-eliminate-rwlock-on-stub-maps)              | Low         | Medium-High |
| P6  | O(n²) transitive eviction in `evict_fqn`                                                                                                                                    | Low         | High        |
|     | **[Indexing](todo/indexing.md)**                                                                                                                                            |             |             |
| X7  | [Recency tracking](todo/indexing.md#x7-recency-tracking)                                                                                                                    | Medium      | Medium-High |
| X6  | Disk cache (evaluate later)                                                                                                                                                 | Medium      | Very High   |
| X16 | [Composer's own class lists bypass `[indexing] exclude`](todo/indexing.md#x16-composers-own-class-lists-bypass-indexing-exclude)                                            | Low-Medium  | Low         |
| X13 | [Decide how workspace-wide edits treat excluded files](todo/indexing.md#x13-decide-how-workspace-wide-edits-treat-excluded-files)                                           | Low-Medium  | Medium      |
| X2  | Parallel file processing — remaining work                                                                                                                                   | Low-Medium  | Medium-High |
| X12 | [Say when an exclude hid the class a diagnostic names](todo/indexing.md#x12-say-when-an-exclude-hid-the-class-a-diagnostic-names)                                           | Low-Medium  | Medium-High |
|     | **[Inline Completion](todo/inline-completion.md)**                                                                                                                          |             |             |
| N1  | Template engine (type-aware snippets)                                                                                                                                       | Medium      | Medium      |
| N2  | N-gram prediction from PHP corpus                                                                                                                                           | Medium      | Very High   |
| N3  | Fine-tuned GGUF sidecar model                                                                                                                                               | Medium      | Very High   |
