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

## Sprint 6 — 0.10.0 release, full indexing

| #    | Item                                                                                                                                                      | Impact      | Complexity  |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
|      | **Release 0.10.0**                                                                                                                                        |             |             |

## Sprint 7 — 1.0 release & IDE extensions

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
|     | Clear [refactoring gate](todo/refactor.md)                                                                                                                      | —           | —           |
| E1  | [External stub packages (ide-helper, etc.)](todo/external-stubs.md#e1-project-level-phpstorm-stubs-for-gtd)                                                     | Medium-High | Low         |
| E5  | [Extension stub coverage audit](todo/external-stubs.md#e5-extension-stub-selection-stubs-extensions)                                                            | Medium      | Low         |
| E4  | [Embedded stub override with external stubs](todo/external-stubs.md#e4-embedded-stub-override-with-external-stubs) (depends on E1)                              | Medium      | Low         |
| E3  | [IDE-provided and `.phpantom.toml` stub paths](todo/external-stubs.md#e3-ide-provided-and-phpantomtoml-stub-paths) (depends on E2)                              | Low-Medium  | Low         |
| D10 | [PHPMD diagnostic proxy](todo/diagnostics.md#d10-phpmd-diagnostic-proxy)                                              | Low        | Medium |
| L1  | [Facade completion](todo/laravel.md#l1-facade-completion-upstream-method-generator-improvement) (upstream `facade-documenter` PRs)                              | High        | High        |
| E2  | [Project-level stubs as type resolution source](todo/external-stubs.md#e2-project-level-stubs-as-resolution-source) (depends on E1)                             | Medium      | High        |
| F20 | [Migrate to the maintained `tower-lsp` fork](todo/lsp-features.md#f20-migrate-to-the-maintained-tower-lsp-fork)                                              | Low-Medium  | Very High   |
| F21 | [Static `typeHierarchyProvider` advertisement](todo/lsp-features.md#f21-static-typehierarchyprovider-advertisement-depends-on-f20) (depends on F20; also needs an upstream `lsp-types` fix) | Low-Medium  | Low         |
|     | **Release 1.0.0 + IDE extensions**                                                                                                                              |             |             |

## Sprint 8 — Blade support

| #    | Item                                                                                                                       | Impact     | Complexity |
| ---- | --------------------------------------------------------------------------------------------------------------------------- | ---------- | ---------- |
|      | Clear [refactoring gate](todo/refactor.md)                                                                                | —          | —          |
| BL23 | [Unbalanced component tag diagnostics](todo/blade.md#bl23-unbalanced-component-tag-diagnostics)                            | Low-Medium | Medium     |
| BL14 | [Folding ranges for Blade files](todo/blade.md#bl14-folding-ranges-for-blade-files)                                        | Low-Medium | Medium     |
| BL24 | [Named slot variables scoped to the component that receives them](todo/blade.md#bl24-named-slot-variables-scoped-to-the-component-that-receives-them) | Low-Medium | Medium |
| BL17 | [`format --check` CLI subcommand for CI](todo/blade.md#bl17-format-check-cli-subcommand-for-ci) (depends on BL16)         | Low-Medium | Medium     |
| BL1  | [Blade-aware code actions](todo/blade.md#bl1-blade-aware-code-actions)                                                      | Medium     | Medium-High |
| BL15 | [Document outline (symbols) for Blade files](todo/blade.md#bl15-document-outline-symbols-for-blade-files)                   | Low-Medium | Medium-High |
| BL16 | [Blade-aware formatting](todo/blade.md#bl16-blade-aware-formatting)                                                          | Low-Medium | High       |

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
| C4  | Non-array functions with dynamic return types                                                                                                                               | Low         | High        |
|     | **[Type Inference](todo/type-inference.md)**                                                                                                                                |             |             |
| T20 | [Type narrowing reconciliation engine](todo/type-inference.md#t20-type-narrowing-reconciliation-engine) (CNF clause algebra, sure/sureNot tracking)                         | Medium-High | Very High   |
| T41 | [`@param-out` is parsed but never read](todo/type-inference.md#t41-param-out-is-parsed-but-never-read)                                                                      | Medium      | Medium      |
| T28 | [Template inference depth priority (shallowest bound wins)](todo/type-inference.md#t28-template-inference-depth-priority-shallowest-bound-wins)                             | Medium      | Medium-High |
| T3  | [Property hooks (PHP 8.4)](todo/type-inference.md#t3-property-hooks-php-84)                                                                                                 | Medium      | Medium-High |
| T34 | [`static::CONST` over-narrows to the declaring class's value](todo/type-inference.md#t34-staticconst-over-narrows-to-the-declaring-classs-value)                            | Medium      | Medium-High |
| T29 | [Definite vs possible variable existence tracking](todo/type-inference.md#t29-definite-vs-possible-variable-existence-tracking)                                             | Medium      | High        |
| T30 | [Literal type collapse limit](todo/type-inference.md#t30-literal-type-collapse-limit)                                                                                       | Low-Medium  | Medium      |
| T40 | [`pathinfo()` returns a shape or a string depending on the flags argument](todo/type-inference.md#t40-pathinfo-returns-a-shape-or-a-string-depending-on-the-flags-argument) | Low-Medium  | Medium      |
| T26 | [Globbed constant unions (`Foo::BAR_*`)](todo/type-inference.md#t26-globbed-constant-unions-foobar_)                                                                        | Low-Medium  | Medium      |
| T33 | [Class constant on an expression (`$obj::CONST`) resolves to nothing](todo/type-inference.md#t33-class-constant-on-an-expression-objconst-resolves-to-nothing)              | Low-Medium  | Medium      |
| T6  | `Closure::bind()` / `Closure::fromCallable()` return type preservation                                                                                                      | Low-Medium  | Medium-High |
| T13 | [Closure variables lose callable signature detail](todo/type-inference.md#t13-closure-variables-lose-callable-signature-detail)                                             | Low-Medium  | Medium-High |
| T31 | [Closure literal-return shape inference](todo/type-inference.md#t31-closure-literal-return-shape-inference)                                                                 | Low-Medium  | Medium-High |
| T4  | [Non-empty-\* type narrowing and propagation](todo/type-inference.md#t4-non-empty--type-narrowing-and-propagation)                                                          | Low-Medium  | High        |
| T5  | Fiber type resolution                                                                                                                                                       | Low         | Medium      |
| T10 | [Ternary expression as RHS of list destructuring](todo/type-inference.md#t10-ternary-expression-as-rhs-of-list-destructuring)                                               | Low         | Medium      |
| T11 | [Nested list destructuring](todo/type-inference.md#t11-nested-list-destructuring)                                                                                           | Low         | Medium      |
|     | **[Bugs](todo/bugs.md)**                                                                                                                                                    |             |             |
|     | **[Diagnostics](todo/diagnostics.md)**                                                                                                                                      |             |             |
| D6  | [Unreachable code diagnostic](todo/diagnostics.md#d6-unreachable-code-diagnostic)                                                                                           | Low-Medium  | Medium      |
| D16 | [`unreachable_match_arm` ignores literal subject types](todo/diagnostics.md#d16-unreachable_match_arm-ignores-literal-subject-types)                                        | Low-Medium  | Medium      |
| D5  | [External tool diagnostic suppression actions](todo/diagnostics.md#d5-external-tool-diagnostic-suppression-actions)                                                         | Low         | Low         |
| D15 | [Unused parameter diagnostic](todo/diagnostics.md#d15-unused-parameter-diagnostic)                                                                                          | Low         | Medium      |
| D17 | [`docblock_native_mismatch` only judges nullability](todo/diagnostics.md#d17-docblock_native_mismatch-only-judges-nullability)                                              | Low         | Medium-High |
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
| F5  | [Call hierarchy](todo/lsp-features.md#f5-call-hierarchy) (incoming/outgoing calls)                                                                                          | Medium      | Medium      |
| F2  | [Partial result streaming via `$/progress`](todo/lsp-features.md#f2-partial-result-streaming-via-progress)                                                                  | Medium      | Medium-High |
| F7  | [Evaluatable expression support (DAP integration)](todo/lsp-features.md#f7-evaluatable-expression-support-dap-integration)                                                  | Low-Medium  | Low         |
| F15 | [Go-to-declaration](todo/lsp-features.md#f15-go-to-declaration)                                                                                                              | Low-Medium  | Low         |
| F14 | [Helix upstream PR](todo/lsp-features.md#f14-helix-upstream-pr) (depends on F13)                                                                                            | Low-Medium  | Low         |
| F16 | [On-type `}` brace de-indent](todo/lsp-features.md#f16-on-type-brace-de-indent)                                                                                            | Low         | Low         |
| F19 | [Connect to a remote/TCP language server](todo/lsp-features.md#f19-connect-to-a-remotetcp-language-server-vs-code-extension)                                               | Low         | Medium      |
|     | **[Signature Help](todo/signature-help.md)**                                                                                                                                |             |             |
| S2  | [Closure / arrow function parameter signature help](todo/signature-help.md#s2-closure-arrow-function-parameter-signature-help)                                             | Medium      | Medium      |
| S3  | Multiple overloaded signatures                                                                                                                                              | Medium      | Medium-High |
| S4  | Named argument awareness in active parameter                                                                                                                                | Low-Medium  | Medium      |
| S5  | Language construct signature help and hover                                                                                                                                 | Low         | Medium      |
|     | **[Laravel](todo/laravel.md)**                                                                                                                                              |             |             |
| L22 | [Broaden recognized call sites for Laravel string keys](todo/laravel.md#l22-broaden-recognized-call-sites-for-laravel-string-keys)                                          | High        | Medium      |
| L24 | [Translation depth: JSON lang files, locales, placeholders](todo/laravel.md#l24-translation-depth-json-lang-files-locales-placeholders)                                     | Medium-High | Medium-High |
| L46 | [`->can()` on a user model the receiver does not name](todo/laravel.md#l46-can-on-a-user-model-the-receiver-does-not-name)                                                  | Medium-High | Medium-High |
| L30 | [Eloquent attribute-array key completion](todo/laravel.md#l30-eloquent-attribute-array-key-completion)                                                                      | Medium      | Medium      |
| L32 | [Config-backed named-resource strings](todo/laravel.md#l32-config-backed-named-resource-strings) (log channels, cache stores, guards, connections, rate limiters)           | Medium      | Medium      |
| L49 | [Unguarded Eloquent mass assignment diagnostic](todo/laravel.md#l49-unguarded-eloquent-mass-assignment-diagnostic)                                                          | Medium      | Medium      |
| L17 | [Additional string contexts without booting](todo/laravel.md#l17-additional-string-contexts-without-booting) (middleware, assets, validation, Inertia)                     | Medium      | Medium-High |
| L31 | [String-key rename, highlight, and semantic tokens](todo/laravel.md#l31-string-key-rename-highlight-and-semantic-tokens)                                                    | Low-Medium  | Medium      |
| L42 | [Morph alias completion in array positions](todo/laravel.md#l42-morph-alias-completion-in-array-positions)                                                                  | Low-Medium  | Medium      |
| L3  | `$dates` array (deprecated)                                                                                                                  | Low-Medium  | Medium      |
| L12 | [`HasUuids` / `HasUlids` trait — `$id` typed as `string`](todo/laravel.md#l12-hasuuids-hasulids-trait-id-typed-as-string)                                                 | Low-Medium  | Medium      |
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
|     | **[Blade](todo/blade.md)**                                                                                                                                                  |             |             |
| BL11 | [Custom directive discovery](todo/blade.md#bl11-custom-directive-discovery) (`Blade::directive()` / `Blade::if()` registrations)                                              | Medium      | Medium      |
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
| P17 | [`mago-names` resolution on the parse hot path](todo/performance.md#p17-mago-names-resolution-on-the-parse-hot-path)                                                        | Medium      | High        |
| P18 | [Subtype result caching](todo/performance.md#p18-subtype-result-caching) (per-request HashMap for hierarchy walks)                                                          | Medium      | High        |
| P47 | [The resolved-class cache lock caps concurrent class resolution](todo/performance.md#p47-the-resolved-class-cache-lock-caps-concurrent-class-resolution)                     | Medium      | Very High   |
| P20 | [Content-hash gated resolution cache persistence](todo/performance.md#p20-content-hash-gated-resolution-cache-persistence)                                                  | Medium      | Very High   |
| P21 | [Offset-shifting for cached diagnostics on partial edits](todo/performance.md#p21-offset-shifting-for-cached-diagnostics-on-partial-edits)                                  | Medium      | Very High   |
| P3  | Parallel pre-filter in `find_implementors`                                                                                                                                  | Low-Medium  | Medium-High |
| P50 | [Cache the top-level scope for `global` keyword resolution](todo/performance.md#p50-cache-the-top-level-scope-for-global-keyword-resolution)                                 | Low-Medium  | High        |
| P48 | [Higher-order collection proxy injection repeats work](todo/performance.md#p48-higher-order-collection-proxy-injection-repeats-work)                                        | Low         | Medium      |
| P49 | [A very long method chain costs superlinear time to analyse](todo/performance.md#p49-a-very-long-method-chain-costs-superlinear-time-to-analyse)                              | Low         | Medium      |
| P15 | [Two-phase stub index construction (eliminate `RwLock` on stub maps)](todo/performance.md#p15-two-phase-stub-index-construction-eliminate-rwlock-on-stub-maps)              | Low         | Medium-High |
| P6  | O(n²) transitive eviction in `evict_fqn`                                                                                                                                    | Low         | High        |
|     | **[Indexing](todo/indexing.md)**                                                                                                                                            |             |             |
| X7  | [Recency tracking](todo/indexing.md#x7-recency-tracking)                                                                                                                    | Medium      | Medium-High |
| X6  | Disk cache (evaluate later)                                                                                                                                                 | Medium      | Very High   |
| X2  | Parallel file processing — remaining work                                                                                                                                   | Low-Medium  | Medium-High |
| X9  | [Honor editor file excludes and PHP associations during indexing](todo/indexing.md#x9-honor-editor-file-excludes-and-php-associations-during-indexing)                      | Low-Medium  | Medium-High |
|     | **[Inline Completion](todo/inline-completion.md)**                                                                                                                          |             |             |
| N1  | Template engine (type-aware snippets)                                                                                                                                       | Medium      | Medium      |
| N2  | N-gram prediction from PHP corpus                                                                                                                                           | Medium      | Very High   |
| N3  | Fine-tuned GGUF sidecar model                                                                                                                                               | Medium      | Very High   |
