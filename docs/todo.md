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
| D25  | [Two traits declaring the same property with different types is not reported](todo/diagnostics.md#d25-two-traits-declaring-the-same-property-with-different-types-is-not-reported) | Low | Low-Medium |
| D24  | [A `match` that does not cover every enum case is not reported](todo/diagnostics.md#d24-a-match-that-does-not-cover-every-enum-case-is-not-reported) (depends on T44) | Medium | Medium |
| D27  | [Destructuring offsets an array cannot have is not reported](todo/diagnostics.md#d27-destructuring-offsets-an-array-cannot-have-is-not-reported) | Low | Medium |
| D26  | [Reading a typed property that nothing initialises is not reported](todo/diagnostics.md#d26-reading-a-typed-property-that-nothing-initialises-is-not-reported) | Low-Medium | Medium-High |
| L1  | [Facade completion](todo/laravel.md#l1-facade-completion-upstream-method-generator-improvement) (upstream `facade-documenter` PRs)                              | High        | High        |
| E2  | [Project-level stubs as type resolution source](todo/external-stubs.md#e2-project-level-stubs-as-resolution-source) (depends on E1)                             | Medium      | High        |
| F20 | [Migrate to the maintained `tower-lsp` fork](todo/lsp-features.md#f20-migrate-to-the-maintained-tower-lsp-fork)                                              | Low-Medium  | Very High   |
| F21 | [Static `typeHierarchyProvider` advertisement](todo/lsp-features.md#f21-static-typehierarchyprovider-advertisement-depends-on-f20) (depends on F20; also needs an upstream `lsp-types` fix) | Low-Medium  |
| BL1  | [Blade-aware code actions](todo/blade.md#bl1-blade-aware-code-actions)                                                      | Medium     | Medium-High | Low         |
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
| T26 | [Class constants named as docblock types (`Foo::BAR`, `Foo::BAR_*`)](todo/type-inference.md#t26-class-constants-named-as-docblock-types-foobar-foobar_) | Medium      | Medium      |
| T34 | [`static::CONST` over-narrows to the declaring class's value](todo/type-inference.md#t34-staticconst-over-narrows-to-the-declaring-classs-value)                            | Medium      | Medium-High |
| T29 | [Definite vs possible variable existence tracking](todo/type-inference.md#t29-definite-vs-possible-variable-existence-tracking)                                             | Medium      | High        |
| T44 | [A single enum case has no type](todo/type-inference.md#t44-a-single-enum-case-has-no-type)                                                                                 | Medium      | High        |
| T46 | [Template arguments are not inferred from how the object is used later](todo/type-inference.md#t46-template-arguments-are-not-inferred-from-how-the-object-is-used-later) | Medium      | High        |
| T47 | [A closure assigned to a variable gets no parameter types from where it is used](todo/type-inference.md#t47-a-closure-assigned-to-a-variable-gets-no-parameter-types-from-where-it-is-used) | Medium      | High        |
| T41 | [A reading of the callee's body overrides a declared `@param-out`](todo/type-inference.md#t41-a-reading-of-the-callees-body-overrides-a-declared-param-out)            | Low-Medium  | Low         |
| T43 | [`self::TypeAlias` inside `@extends`'s generic argument is not resolved](todo/type-inference.md#t43-selftypealias-inside-extendss-generic-argument-is-not-resolved) | Low         | Medium      |
|     | **[Diagnostics](todo/diagnostics.md)**                                                                                                                                      |             |             |
| D16 | [`unreachable_match_arm` ignores literal subject types](todo/diagnostics.md#d16-unreachable_match_arm-ignores-literal-subject-types)                                        | Low-Medium  | Medium      |
| D5  | [External tool diagnostic suppression actions](todo/diagnostics.md#d5-external-tool-diagnostic-suppression-actions)                                                         | Low         | Low         |
| D15 | [Unused parameter diagnostic](todo/diagnostics.md#d15-unused-parameter-diagnostic)                                                                                          | Low         | Medium      |
| D17 | [`docblock_native_mismatch` only judges nullability](todo/diagnostics.md#d17-docblock_native_mismatch-only-judges-nullability)                                              | Low         | Medium-High |
| D18 | [`array<int, T>` is accepted wherever a `list<T>` is declared](todo/diagnostics.md#d18-arrayint-t-is-accepted-wherever-a-listt-is-declared)                                 | Low         | Medium-High |
| D19 | [`invalid_member_access` cannot tell a property read from a write](todo/diagnostics.md#d19-invalid_member_access-cannot-tell-a-property-read-from-a-write) (includes PHP 8.4 `private(set)`) | Medium      | Medium      |
| D20 | [`Foo::$bar` and `Foo::bar` are the same span](todo/diagnostics.md#d20-foobar-and-foobar-are-the-same-span)                                                                  | Low         | Medium      |
| D21 | [A union of an unreachable and a missing member is reported by neither check](todo/diagnostics.md#d21-a-union-of-an-unreachable-and-a-missing-member-is-reported-by-neither-check) | Low         | Medium-High |
| D22 | [Member provenance is recomputed instead of recorded](todo/diagnostics.md#d22-member-provenance-is-recomputed-instead-of-recorded)                                        | Medium      | Medium-High |
| D23 | [A rebound closure's scope is added to the lexical one rather than replacing it](todo/diagnostics.md#d23-a-rebound-closures-scope-is-added-to-the-lexical-one-rather-than-replacing-it) | Low-Medium  | Medium      |
| D28 | ["Remove unreachable code" is wired to PHPStan only](todo/diagnostics.md#d28-remove-unreachable-code-is-wired-to-phpstan-only)                                              | Low-Medium  | Medium      |
| D29 | [`namespace` and `declare` bodies break the reachability flow](todo/diagnostics.md#d29-namespace-and-declare-bodies-break-the-reachability-flow)                              | Low         | Low-Medium  |
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
| L46 | [`->can()` on a user model the receiver does not name](todo/laravel.md#l46-can-on-a-user-model-the-receiver-does-not-name)                                                  | Medium-High | Medium-High |
| L30 | [Eloquent attribute-array key completion](todo/laravel.md#l30-eloquent-attribute-array-key-completion)                                                                      | Medium      | Medium      |
| L56 | [Typed Laravel connection names](todo/laravel.md#l56-typed-laravel-connection-names)                                                                                        | Medium      | Medium      |
| L58 | [Laravel rate limiter names](todo/laravel.md#l58-laravel-rate-limiter-names)                                                                                                | Medium      | Medium      |
| L57 | [Laravel queue names](todo/laravel.md#l57-laravel-queue-names)                                                                                                              | Low-Medium  | Medium      |
| L59 | [Typed controller middleware names](todo/laravel.md#l59-typed-controller-middleware-names)                                                                                  | Low-Medium  | Medium      |
| L53 | [Collection key types from the column for `keyBy` / `groupBy` / `pluck`](todo/laravel.md#l53-collection-key-types-from-the-column-for-keyby-groupby-pluck)                   | Medium      | Medium      |
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
| P52 | [The diagnostic benchmarks measure a path no consumer takes](todo/performance.md#p52-the-diagnostic-benchmarks-measure-a-path-no-consumer-takes)                            | Medium      | Low         |
| P53 | [Diagnostics and type-hint resolution deep-copy classes they only read](todo/performance.md#p53-diagnostics-and-type-hint-resolution-deep-copy-classes-they-only-read)     | Medium      | Low         |
| P51 | [CI checks for how cost grows with input size](todo/performance.md#p51-ci-checks-for-how-cost-grows-with-input-size)                                                      | Medium      | Low-Medium  |
| P75 | [Closures and calls through a variable on a long file rescan it](todo/performance.md#p75-closures-and-calls-through-a-variable-on-a-long-file-rescan-it)                 | Medium      | Medium      |
| P21 | [Offset-shifting for cached diagnostics on partial edits](todo/performance.md#p21-offset-shifting-for-cached-diagnostics-on-partial-edits)                                  | Medium      | Very High   |
| P73 | [A lazily loaded file is parsed three times](todo/performance.md#p73-a-lazily-loaded-file-is-parsed-three-times)                                                          | Low-Medium  | Low         |
| P74 | [Every class lookup by name re-parses and lowercases the name](todo/performance.md#p74-every-class-lookup-by-name-re-parses-and-lowercases-the-name)                      | Low-Medium  | Low         |
| P66 | [Stub version filtering rescans a stub file once per symbol it declares](todo/performance.md#p66-stub-version-filtering-rescans-a-stub-file-once-per-symbol-it-declares) | Low-Medium  | Low-Medium  |
| P58 | [A member-completion cache hit copies the whole item list](todo/performance.md#p58-a-member-completion-cache-hit-copies-the-whole-item-list)                                | Low         | Low         |
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
