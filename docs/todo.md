# PHPantom — Roadmap

This document tracks planned work for PHPantom. Each item links to a
domain document with full context. Items are grouped into time-boxed
sprints (roughly 1-2 weeks each) and a backlog of ideas not yet
scheduled.

**Guiding priorities:** Completion accuracy → Type intelligence →
Cross-file navigation → Diagnostics → Code actions → Performance.

Items inside each sprint are ordered by priority (top = do first):
quick wins (low effort) before heavy lifts, dependencies before their
dependents, and within the same effort tier by impact descending.
The backlog is ordered by impact (descending), then effort (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Effort** | **Low** (≤ 1 day), **Medium** (2-5 days), **Medium-High** (1-2 weeks), **High** (2-4 weeks), **Very High** (> 1 month) |

# Scheduled Sprints

## Sprint 5 — 0.9.0 release

| #   | Item                                                                                                                  | Impact     | Effort |
| --- | --------------------------------------------------------------------------------------------------------------------- | ---------- | ------ |

## Sprint 6 — 0.10.0 release, editor plugins & type intelligence

| #   | Item                                                                                                                  | Impact     | Effort |
| --- | --------------------------------------------------------------------------------------------------------------------- | ---------- | ------ |
|     | Clear [refactoring gate](todo/refactor.md)                                                                                                                      | —           | —           |
| L21 | [Tighten the supertype-where-subtype comparison escape hatch (blocked on resolver precision)](todo/laravel.md#l21-tighten-the-supertype-where-subtype-comparison-escape-hatch-blocked-on-resolver-precision) | Medium      | High        |
| L18 | [`Macroable::mixin()` registrations](todo/laravel.md#l18-macroablemixin-registrations)                                                                          | Low         | Medium      |
| X4  | [Full background indexing](todo/indexing.md#x4-full-background-indexing) (workspace symbols, fast find-references)                                              | Medium      | High        |
| L1  | [Facade completion](todo/laravel.md#l1-facade-completion--upstream-method-template-improvement)                                                                 | High        | High        |
| D10 | [PHPMD diagnostic proxy](todo/diagnostics.md#d10-phpmd-diagnostic-proxy)                                              | Low        | Medium |

## Sprint 7 — 1.0 release & IDE extensions

| #   | Item                                                                                                                                                            | Impact      | Effort      |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
|     | Clear [refactoring gate](todo/refactor.md)                                                                                                                      | —           | —           |
| E5  | [Extension stub coverage audit](todo/external-stubs.md#e5-extension-stub-selection-stubs-extensions)                                                            | Medium      | Low         |
| E1  | [External stub packages (ide-helper, etc.)](todo/external-stubs.md#e1-project-level-phpstorm-stubs-for-gtd)                                                     | Medium-High | Medium      |
| E2  | [Project-level stubs as type resolution source](todo/external-stubs.md#e2-project-level-stubs-as-resolution-source) (depends on E1)                             | Medium      | Medium      |
| E3  | [IDE-provided and `.phpantom.toml` stub paths](todo/external-stubs.md#e3-ide-provided-and-phpantomtoml-stub-paths) (depends on E2)                              | Low-Medium  | Low         |
| E4  | [Stub version alignment with target PHP](todo/external-stubs.md#e4-embedded-stub-override-with-external-stubs) (depends on E1)                                  | Medium      | Medium      |
|     | **Release 1.0.0 + IDE extensions**                                                                                                                              |             |             |

## Sprint 8 — Blade support

| #   | Item                                                                                                                      | Impact | Effort |
| --- | ------------------------------------------------------------------------------------------------------------------------- | ------ | ------ |
|     | Clear [refactoring gate](todo/refactor.md)                                                                                | —      | —      |
| BL1 | [Blade-aware code actions](todo/blade.md#8-blade-aware-code-actions)                                                      | Medium | Medium |
| BL2 | [Template and component file discovery](todo/blade.md#9-template-and-component-file-discovery)                            | High   | Medium |
| BL3 | [Component tag parsing (`<x-...>`, `<livewire:...>`, `@props`)](todo/blade.md#10-x-component-tag-parsing-in-preprocessor) | High   | High   |
| BL4 | [Component and view name completion](todo/blade.md#13-component-and-view-name-completion)                                 | High   | Medium |
| BL5 | [Go-to-definition for view names and components](todo/blade.md#15-go-to-definition-for-view-names-and-components)         | Medium | Medium |
| BL6 | [`@extends` signature merging and component class typing](todo/blade.md#16-signature-merging-for-extends)                 | Medium | High   |
| BL7 | [Blade directive completion](todo/blade.md#19-directive-name-completion)                                                  | Medium | Low    |

# Backlog

Items not yet assigned to a sprint. Worth doing eventually but
unlikely to move the needle for most users.

| #   | Item                                                                                                                                                                        | Impact      | Effort      |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
|     | **[Completion](todo/completion.md)**                                                                                                                                        |             |             |
| C1  | Array functions needing new code paths                                                                                                                                      | Medium      | High        |
| C9  | [Lazy documentation via `completionItem/resolve`](todo/completion.md#c9-lazy-documentation-via-completionitemresolve)                                                       | Medium      | Medium      |
| C11 | [Smarter member ordering after `->` / `::`](todo/completion.md#c11-smarter-member-ordering-after----)                                                                       | Medium      | Medium      |
| C8  | [Filesystem proximity as an affinity tiebreaker](todo/completion.md#c8-filesystem-proximity-as-an-affinity-tiebreaker)                                                      | Low-Medium  | Low         |
| C3  | Go-to-definition for array shape keys via bracket access                                                                                                                    | Low-Medium  | Medium      |
| C7  | `class_alias()` support                                                                                                                                                     | Low-Medium  | Medium      |
| C4  | Non-array functions with dynamic return types                                                                                                                               | Low         | High        |
| C5  | `#[ReturnTypeContract]` parameter-dependent return types                                                                                                                    | Low         | Low         |
| C6  | `#[ExpectedValues]` parameter value suggestions                                                                                                                             | Low         | Medium      |
| C10 | [Deprecation markers on class-name completions from all sources](todo/completion.md#c10-deprecation-markers-on-class-name-completions-from-all-sources)                     | Low         | Low         |
|     | **[Type Inference](todo/type-inference.md)**                                                                                                                                |             |             |
| T20 | [Type narrowing reconciliation engine](todo/type-inference.md#t20-type-narrowing-reconciliation-engine) (CNF clause algebra, sure/sureNot tracking)                         | Medium-High | High        |
| T27 | [Per-expression type caching during forward walk](todo/type-inference.md#t27-per-expression-type-caching-during-forward-walk)                                               | Medium-High | Medium      |
| T2  | [File system watching for vendor and project changes](todo/type-inference.md#t2-file-system-watching-for-vendor-and-project-changes)                                        | Medium-High | Medium      |
| T28 | [Template inference depth priority (shallowest bound wins)](todo/type-inference.md#t28-template-inference-depth-priority-shallowest-bound-wins)                             | Medium      | Low-Medium  |
| T29 | [Definite vs possible variable existence tracking](todo/type-inference.md#t29-definite-vs-possible-variable-existence-tracking)                                             | Medium      | Medium      |
| T3  | [Property hooks (PHP 8.4)](todo/type-inference.md#t3-property-hooks-php-84)                                                                                                 | Medium      | Medium      |
| T25 | [Call-site template argument inference for callable parameters](todo/type-inference.md#t25-call-site-template-argument-inference-for-callable-parameters) (partially done)  | Medium      | Medium      |
| T30 | [Literal type collapse limit](todo/type-inference.md#t30-literal-type-collapse-limit)                                                                                       | Low-Medium  | Low         |
| T24 | [`stdClass` dynamic property access](todo/type-inference.md#t24-stdclass-dynamic-property-access)                                                                           | Low-Medium  | Low         |
| T6  | `Closure::bind()` / `Closure::fromCallable()` return type preservation                                                                                                      | Low-Medium  | Low-Medium  |
| T13 | [Closure variables lose callable signature detail](todo/type-inference.md#t13-closure-variables-lose-callable-signature-detail)                                             | Low-Medium  | Medium      |
| T26 | [Globbed constant unions (`Foo::BAR_*`)](todo/type-inference.md#t26-globbed-constant-unions-foobar_)                                                                        | Low-Medium  | Medium      |
| T31 | [Closure literal-return shape inference](todo/type-inference.md#t31-closure-literal-return-shape-inference)                                                                 | Low-Medium  | Medium      |
| T4  | Non-empty-\* type narrowing and propagation                                                                                                                                 | Low         | Low         |
| T5  | Fiber type resolution                                                                                                                                                       | Low         | Low         |
| T9  | [Dead-code elimination after `never`-returning calls](todo/type-inference.md#t9-dead-code-elimination-after-never-returning-calls)                                          | Low         | Low-Medium  |
| T10 | [Ternary expression as RHS of list destructuring](todo/type-inference.md#t10-ternary-expression-as-rhs-of-list-destructuring)                                               | Low         | Low-Medium  |
| T11 | [Nested list destructuring](todo/type-inference.md#t11-nested-list-destructuring)                                                                                           | Low         | Low-Medium  |
|     | **[Diagnostics](todo/diagnostics.md)**                                                                                                                                      |             |             |
| D14 | [Tighten argument type mismatch diagnostic (Phase 2)](todo/diagnostics.md#d14-tighten-argument-type-mismatch-diagnostic-phase-2)                                            | High        | Medium      |
| D13 | [Unify diagnostic subject resolution with completion/hover](todo/diagnostics.md#d13-unify-diagnostic-subject-resolution-with-completionhover)                              | Medium      | Medium      |
| D6  | [Unreachable code diagnostic](todo/diagnostics.md#d6-unreachable-code-diagnostic)                                                                                           | Low-Medium  | Low         |
| D3  | [Deprecated rendering — chain subject resolution](todo/diagnostics.md#d3-deprecated-rendering--chain-subject-resolution)                                                    | Low-Medium  | Medium      |
| D5  | [External tool diagnostic suppression actions](todo/diagnostics.md#d5-external-tool-diagnostic-suppression-actions)                                                         | Low         | Low         |
| D15 | [Unused parameter diagnostic](todo/diagnostics.md#d15-unused-parameter-diagnostic)                                                                                          | Low         | Low         |
|     | **[Bug Fixes](todo/bugs.md)**                                                                                                                                               |             |             |
|     | **[Code Actions](todo/actions.md)**                                                                                                                                         |             |             |
| A40 | [Generate method from call](todo/actions.md#a40-generate-method-from-call)                                                                                                  | Medium-High | Medium      |
| A41 | [Create class from non-existing name](todo/actions.md#a41-create-class-from-non-existing-name)                                                                              | Medium      | Medium      |
| A16 | [Snippet placeholder for extracted method name](todo/actions.md#a16-snippet-placeholder-for-extracted-method-name) (lets the user type over the generated name immediately) | Medium      | Low-Medium  |
| A25 | [`strpos` → `str_contains`](todo/actions.md#a25-strpos--str_contains-php-80) (PHP 8.0+)                                                                                     | Medium      | Low         |
| A28 | [Explicit nullable parameter type](todo/actions.md#a28-explicit-nullable-parameter-type-php-84-deprecation) (PHP 8.4 deprecation)                                           | Medium      | Low         |
| A29 | [Simplify boolean return](todo/actions.md#a29-simplify-boolean-return) (`if (cond) return true; return false;` → `return cond;`)                                            | Low-Medium  | Medium      |
| A31 | [Remove always-else](todo/actions.md#a31-remove-always-else-extract-guard-clause) (extract guard clause)                                                                    | Low-Medium  | Medium      |
| A34 | [Unified code action handler architecture](todo/actions.md#a34-unified-code-action-handler-architecture) (closure-based resolve, unified fix type)                          | Medium      | Medium-High |
| A37 | [Simplify with `?->`](todo/actions.md#a37-simplify-with---nullsafe-operator) (replace null-checked chains with the nullsafe operator)                                       | Low-Medium  | Medium      |
| A38 | [Convert if/elseif chain to switch](todo/actions.md#a38-convert-ifelseif-chain-to-switch)                                                                                   | Low-Medium  | Medium      |
| A39 | [Convert to string interpolation](todo/actions.md#a39-convert-to-string-interpolation) (`'Hello ' . $name` → `"Hello $name"`)                                               | Low         | Low         |
| A43 | [Update docblock generics](todo/actions.md#a43-update-docblock-generics)                                                                                                    | Low         | Low-Medium  |
|     | **[PHPStan Code Actions](todo/phpstan-actions.md)**                                                                                                                         |             |             |
| H4  | `assign.byRefForeachExpr` — unset by-reference foreach variable                                                                                                             | Medium      | Medium      |
| H13 | `property.notFound` — declare missing property (same-class)                                                                                                                 | Medium      | Medium      |
| H15 | Template bound from tip — add `@template T of X`                                                                                                                            | Medium      | Medium      |
| H16 | `match.unhandled` — add missing match arms                                                                                                                                  | Medium      | Medium      |
| H19 | `property.unused` / `method.unused` — remove unused member                                                                                                                  | Low         | Low         |
| H20 | `generics.callSiteVarianceRedundant` — remove redundant variance annotation                                                                                                 | Low         | Low         |
| H23 | `instanceof.alwaysTrue` — remove redundant instanceof check                                                                                                                 | Low         | Low         |
| H24 | `catch.neverThrown` — remove unnecessary catch clause                                                                                                                       | Low         | Low         |
|     | **[CLI Fix Rules](todo/fix-cli.md)**                                                                                                                                        |             |             |
| FX1 | [`deprecated` — replace deprecated symbol usage](todo/fix-cli.md#fx1-deprecated--replace-deprecated-symbol-usage)                                                           | Medium      | Medium      |
| FX2 | [`unused_variable` — remove unused variables](todo/fix-cli.md#fx2-unused_variable--remove-unused-variables)                                                                 | Medium      | Medium      |
| FX3 | [`phpstan.return.unusedType` — remove unused type from return union](todo/fix-cli.md#fx3-phpstanreturnunusedtype--remove-unused-type-from-return-union)                     | Medium      | Medium      |
| FX4 | [`phpstan.missingType.iterableValue` — add `@return` with iterable type](todo/fix-cli.md#fx4-phpstanmissingtypeiterablevalue--add-return-with-iterable-type)                | Medium      | Medium      |
| FX5 | [`phpstan.property.unused` / `phpstan.method.unused` — remove unused member](todo/fix-cli.md#fx5-phpstanpropertyunused--phpstanmethodunused--remove-unused-member)          | Low         | Low         |
| FX6 | [`phpstan.generics.callSiteVarianceRedundant` — remove redundant variance](todo/fix-cli.md#fx6-phpstangenericscallsitevarianceredundant--remove-redundant-variance)         | Low         | Low         |
| FX7 | [`add_return_type` — generate `@return` docblocks from function bodies](todo/fix-cli.md#fx7-add_return_type--generate-return-docblocks-from-function-bodies)                | Medium-High | Medium      |
|     | **[LSP Features](todo/lsp-features.md)**                                                                                                                                    |             |             |
| F11 | [VS Code extension](todo/lsp-features.md#f11-vs-code-extension)                                                                                                              | High        | Medium      |
| F12 | [IntelliJ / PHPStorm plugin](todo/lsp-features.md#f12-intellij--phpstorm-plugin)                                                                                            | High        | Medium      |
| F13 | [Homebrew formula](todo/lsp-features.md#f13-homebrew-formula)                                                                                                                | Medium      | Low         |
| F17 | [Class move with reference update](todo/lsp-features.md#f17-class-move-with-reference-update)                                                                               | Medium      | Medium-High |
| F18 | [Fix namespace/class name from PSR-4](todo/lsp-features.md#f18-fix-namespaceclass-name-from-psr-4)                                                                          | Medium      | Medium      |
| F5  | [Call hierarchy](todo/lsp-features.md#f5-call-hierarchy) (incoming/outgoing calls)                                                                                          | Medium      | Medium      |
| F2  | [Partial result streaming via `$/progress`](todo/lsp-features.md#f2-partial-result-streaming-via-progress)                                                                  | Medium      | Medium-High |
| F7  | [Evaluatable expression support (DAP integration)](todo/lsp-features.md#f7-evaluatable-expression-support-dap-integration)                                                  | Low-Medium  | Low         |
| F15 | [Go-to-declaration](todo/lsp-features.md#f15-go-to-declaration)                                                                                                              | Low-Medium  | Low         |
| F14 | [Helix upstream PR](todo/lsp-features.md#f14-helix-upstream-pr) (depends on F13)                                                                                            | Low-Medium  | Low         |
| F3  | [Incremental text sync](todo/lsp-features.md#f3-incremental-text-sync)                                                                                                       | Low-Medium  | Medium      |
| F16 | [On-type `}` brace de-indent](todo/lsp-features.md#f16-on-type--brace-de-indent)                                                                                            | Low         | Low         |
| F8  | [Test ↔ implementation navigation via `@covers`](todo/lsp-features.md#f8-test--implementation-navigation-via-covers)                                                        | Low         | Medium      |
| F19 | [Connect to a remote/TCP language server](todo/lsp-features.md#f19-connect-to-a-remotetcp-language-server-vs-code-extension)                                               | Low         | Low-Medium  |
|     | **[Signature Help](todo/signature-help.md)**                                                                                                                                |             |             |
| S1  | [Attribute constructor signature help](todo/signature-help.md#s1-attribute-constructor-signature-help)                                                                      | Medium      | Medium      |
| S2  | [Closure / arrow function parameter signature help](todo/signature-help.md#s2-closure--arrow-function-parameter-signature-help)                                             | Medium      | Medium      |
| S3  | Multiple overloaded signatures                                                                                                                                              | Medium      | Medium-High |
| S4  | Named argument awareness in active parameter                                                                                                                                | Low-Medium  | Medium      |
| S5  | Language construct signature help and hover                                                                                                                                 | Low         | Low         |
|     | **[Laravel](todo/laravel.md)**                                                                                                                                              |             |             |
| L14 | [Diagnostics for Laravel string keys](todo/laravel.md#l14-diagnostics-for-laravel-string-keys) (route/config/env/trans/view)                                                | High        | Medium      |
| L22 | [Broaden recognized call sites for Laravel string keys](todo/laravel.md#l22-broaden-recognized-call-sites-for-laravel-string-keys)                                          | High        | Low-Medium  |
| L15 | [Completion for Laravel string keys](todo/laravel.md#l15-completion-for-laravel-string-keys)                                                                                | High        | Medium      |
| L5  | [`abort_if`/`abort_unless` type narrowing](todo/laravel.md#l5-abort_ifabort_unless-type-narrowing)                                                                          | High        | Medium      |
| L24 | [Translation depth: JSON lang files, locales, placeholders](todo/laravel.md#l24-translation-depth-json-lang-files-locales-placeholders)                                     | Medium-High | Medium      |
| L38 | [Typed `validated()` array shapes from rules](todo/laravel.md#l38-typed-validated-array-shapes-from-rules)                                                                  | Medium-High | Medium-High |
| L26 | [Gate ability and policy strings](todo/laravel.md#l26-gate-ability-and-policy-strings)                                                                                      | Medium-High | Medium-High |
| L35 | [Migration file parsing](todo/laravel.md#l35-migration-file-parsing) (model columns for unannotated models)                                                                 | Medium-High | High        |
| L16 | [Hover for Laravel string keys](todo/laravel.md#l16-hover-for-laravel-string-keys)                                                                                          | Medium      | Low-Medium  |
| L23 | [Route parameter name completion](todo/laravel.md#l23-route-parameter-name-completion)                                                                                      | Medium      | Low-Medium  |
| L30 | [Eloquent attribute-array key completion](todo/laravel.md#l30-eloquent-attribute-array-key-completion)                                                                      | Medium      | Low-Medium  |
| L32 | [Config-backed named-resource strings](todo/laravel.md#l32-config-backed-named-resource-strings) (log channels, cache stores, guards, connections, rate limiters)           | Medium      | Low-Medium  |
| L17 | [Additional string contexts without booting](todo/laravel.md#l17-additional-string-contexts-without-booting) (middleware, assets, validation, Inertia)                     | Medium      | Medium      |
| L34 | [Schema dump parsing](todo/laravel.md#l34-schema-dump-parsing-databaseschemasql) (model columns from `database/schema/*.sql`)                                               | Medium      | Medium      |
| L36 | [Container binding registrations from service providers](todo/laravel.md#l36-container-binding-registrations-from-service-providers)                                        | Medium      | Medium      |
| L37 | [Request input key completion from validation rules](todo/laravel.md#l37-request-input-key-completion-from-validation-rules)                                                | Medium      | Medium      |
| L25 | [Storage disk name strings](todo/laravel.md#l25-storage-disk-name-strings)                                                                                                  | Low-Medium  | Low         |
| L31 | [String-key rename, highlight, and semantic tokens](todo/laravel.md#l31-string-key-rename-highlight-and-semantic-tokens)                                                    | Low-Medium  | Low-Medium  |
| L29 | [Livewire and Volt component names](todo/laravel.md#l29-livewire-and-volt-component-names)                                                                                  | Low-Medium  | Medium      |
| L33 | [Artisan command and signature strings](todo/laravel.md#l33-artisan-command-and-signature-strings)                                                                          | Low-Medium  | Medium      |
| L3  | `$dates` array (deprecated)                                                                                                                  | Low-Medium  | Low         |
| L12 | [`HasUuids` / `HasUlids` trait — `$id` typed as `string`](todo/laravel.md#l12-hasuuids--hasulids-trait--id-typed-as-string)                                                 | Low-Medium  | Low         |
| L6  | Factory `has*`/`for*` relationship methods                                                                                                                                  | Low-Medium  | Medium      |
| L7  | `$pivot` property on BelongsToMany                                                                                                                                          | Medium      | Medium-High |
| L13 | [Factory count-conditional return types](todo/laravel.md#l13-factory-count-conditional-return-types)                                                                       | Medium      | Medium-High |
| L8  | `withSum`/`withAvg`/`withMin`/`withMax` aggregate properties                                                                                                                | Low-Medium  | Medium-High |
| L9  | Higher-order collection proxies                                                                                                                                             | Low-Medium  | Medium-High |
| L10 | `View::withX()` / `RedirectResponse::withX()` dynamic methods                                                                                                               | Low         | Low         |
| L27 | [Legacy `Controller@method` action strings](todo/laravel.md#l27-legacy-controllermethod-action-strings)                                                                     | Low         | Low         |
| L28 | [Path helper links and completion](todo/laravel.md#l28-path-helper-links-and-completion)                                                                                    | Low         | Low         |
| L39 | [Unused view and translation key detection](todo/laravel.md#l39-unused-view-and-translation-key-detection)                                                                  | Low         | Medium      |
|     | **[Blade](todo/blade.md)**                                                                                                                                                  |             |             |
| BL8 | [Template signature resolution chain](todo/blade.md#21-template-signature-resolution-chain) (Bladestan-compatible contract model)                                           | High        | Medium      |
| BL9 | [`view()` call-site validation](todo/blade.md#24-view-call-site-validation) (diagnostics against template signatures)                                                       | Medium-High | Medium-High |
| BL10 | [Cross-file `@section` / `@stack` name intelligence](todo/blade.md#22-cross-file-section--stack-name-intelligence)                                                         | Medium      | Medium      |
| BL11 | [Custom directive discovery](todo/blade.md#23-custom-directive-discovery) (`Blade::directive()` / `Blade::if()` registrations)                                              | Medium      | Low-Medium  |
| BL12 | [Call-site variable inference](todo/blade.md#25-call-site-variable-inference-late-addition) (late addition, lowest-priority source)                                         | Low-Medium  | Medium-High |
|     | **[External Stubs](todo/external-stubs.md)**                                                                                                                                |             |             |
| E6  | Stub install prompt for non-Composer projects                                                                                                                               | Low         | Low         |
| E7  | [Stub-based framework patches](todo/external-stubs.md#e7-stub-based-framework-patches)                                                                                      | Medium      | Medium      |
|     | **[Performance](todo/performance.md) / [Eager Resolution](todo/eager-resolution.md)**                                                                                       |             |             |
| ER5 | [Mago-style separated metadata](todo/eager-resolution.md#er5--mago-style-separated-metadata)                                                                                | High        | High        |
| P29 | [Migrate to `mago-phpdoc-syntax`](todo/performance.md#p29-migrate-to-mago-phpdoc-syntax) (drop deprecated `mago-docblock` / `mago-type-syntax`)                             | Medium      | Medium      |
| P16 | [Pre-parsed stub format (eliminate raw PHP embedding)](todo/performance.md#p16-pre-parsed-stub-format-eliminate-raw-php-embedding)                                          | High        | Medium-High |
| P25 | [`type_mismatch_argument` / `argument_count_mismatch` slow on large single files](todo/performance.md#p25-type_mismatch_argument--argument_count_mismatch-slow-on-large-single-files) | Medium      | Medium      |
| P22 | [Signature change re-queues slow diagnostics for every open file](todo/performance.md#p22-signature-change-re-queues-slow-diagnostics-for-every-open-file)                  | Medium-High | Medium      |
| P14 | [Eager docblock parsing into structured fields](todo/performance.md#p14-eager-docblock-parsing-into-structured-fields)                                                      | Medium      | Medium      |
| P9  | [`resolved_class_cache` generic-arg specialisation](todo/performance.md#p9-resolved_class_cache-generic-arg-specialisation)                                                 | Medium      | Medium      |
| P27 | [`object`/`?object` call-return check re-resolves the subject a second time](todo/performance.md#p27-objectobject-call-return-check-re-resolves-the-subject-a-second-time)  | Medium      | Low         |
| P11 | [Uncached base-resolution in `build_scope_methods_for_builder`](todo/performance.md#p11-uncached-base-resolution-in-build_scope_methods_for_builder)                        | Low-Medium  | Low         |
| P3  | Parallel pre-filter in `find_implementors`                                                                                                                                  | Low-Medium  | Medium      |
| P6  | O(n²) transitive eviction in `evict_fqn`                                                                                                                                    | Low         | Low         |
| P15 | [Two-phase stub index construction (eliminate `RwLock` on stub maps)](todo/performance.md#p15-two-phase-stub-index-construction-eliminate-rwlock-on-stub-maps)              | Low         | Medium      |
| P17 | [`mago-names` resolution on the parse hot path](todo/performance.md#p17-mago-names-resolution-on-the-parse-hot-path)                                                        | Medium      | Low         |
| P18 | [Subtype result caching](todo/performance.md#p18-subtype-result-caching) (per-request HashMap for hierarchy walks)                                                          | Medium      | Low         |
| P20 | [Content-hash gated resolution cache persistence](todo/performance.md#p20-content-hash-gated-resolution-cache-persistence)                                                  | Medium      | Medium      |
| P21 | [Offset-shifting for cached diagnostics on partial edits](todo/performance.md#p21-offset-shifting-for-cached-diagnostics-on-partial-edits)                                  | Medium      | Medium      |
| P23 | [`workspace/symbol` lowercases every symbol name per request](todo/performance.md#p23-workspacesymbol-allocates-a-lowercase-copy-of-every-symbol-name-per-request)          | Low-Medium  | Low         |
| P28 | [`process_assert_narrowing` clones the scope per variable for every statement](todo/performance.md#p28-process_assert_narrowing-clones-the-top-level-scope-once-per-variable-for-every-statement) | Low-Medium  | Low         |
|     | **[Indexing](todo/indexing.md)**                                                                                                                                            |             |             |
| X3  | Completion item detail on demand (`completionItem/resolve`)                                                                                                                 | Medium      | Medium      |
| X7  | [Recency tracking](todo/indexing.md#x7-recency-tracking)                                                                                                                    | Medium      | Medium      |
| X2  | Parallel file processing — remaining work                                                                                                                                   | Low-Medium  | Medium      |
| X5  | Granular progress reporting for indexing, GTI, and Find References                                                                                                          | Low-Medium  | Medium      |
| X8  | [Inverted reference index for O(k) find-references](todo/indexing.md#x8-inverted-reference-index-for-ok-find-references)                                                    | Medium-High | Medium      |
| X9  | [Honor editor file excludes and PHP associations during indexing](todo/indexing.md#x9-honor-editor-file-excludes-and-php-associations-during-indexing)                      | Low-Medium  | Medium      |
| X6  | Disk cache (evaluate later)                                                                                                                                                 | Medium      | High        |
|     | **[Inline Completion](todo/inline-completion.md)**                                                                                                                          |             |             |
| N1  | Template engine (type-aware snippets)                                                                                                                                       | Medium      | High        |
| N2  | N-gram prediction from PHP corpus                                                                                                                                           | Medium      | Very High   |
| N3  | Fine-tuned GGUF sidecar model                                                                                                                                               | Medium      | Very High   |
