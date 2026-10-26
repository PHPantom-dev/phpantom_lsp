# PHPantom — Roadmap

This document tracks planned work for PHPantom. Each item links to a
domain document with full context. Items are grouped into time-boxed
sprints (roughly 1-2 weeks each) and feature sets: one per framework,
and one per area of the language server for everything else.

**Guiding priorities:** Completion accuracy → Type intelligence →
Cross-file navigation → Diagnostics → Code actions → Performance.

Items inside each sprint and feature set are ordered by priority (top =
do first): low-complexity items (easiest to assign, least
implementation risk) before heavy lifts, dependencies before their
dependents, and within the same complexity tier by impact descending.

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
|     | **Release 1.0.0 + IDE extensions**                                                                                                                              |             |             |

# Feature Sets

A feature set is the work toward one goal: making PHPantom fully usable
for a framework, or improving one area of the language server. Feature
sets are not time-boxed and not ordered against each other or the
sprints; one is taken up when its users ask for it, and items move from
a set into a sprint when they are scheduled. A framework's set opens
with a demo project and a baseline from real code, so the support is
tested against what its users write rather than built on spec.

An item may appear in more than one sprint or feature set when several
depend on it (F59 serves Symfony and Drupal). When it
ships, remove every row that names it.

## Symfony & Twig

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| F59 | [Send Twig, YAML, and XML documents to the server from Zed](todo/lsp-features.md#f59-send-twig-yaml-and-xml-documents-to-the-server-from-zed) (Zed PR) | Medium | Low |
| P86 | [A Symfony app's generated `config/reference.php` is slow to parse](todo/performance.md#p86-a-symfony-apps-generated-configreferencephp-is-slow-to-parse) | Medium | Medium |
| F35 | [Suggest a close name for an unknown template or view](todo/lsp-features.md#f35-suggest-a-close-name-for-an-unknown-template-or-view) | Low-Medium  | Low         |
| F27 | [Symfony route parameters](todo/lsp-features.md#f27-symfony-route-parameters) (blocks full Symfony support) | Medium      | Medium      |
| F29 | [Report Symfony translation keys nothing declares](todo/lsp-features.md#f29-report-symfony-translation-keys-nothing-declares) (blocks full Symfony support) | Medium      | Medium      |
| F32 | [Report unknown Symfony names in Twig templates](todo/lsp-features.md#f32-report-unknown-symfony-names-in-twig-templates) (blocks full Twig support) | Medium      | Medium      |
| F36 | [Twig block navigation](todo/lsp-features.md#f36-twig-block-navigation) (blocks full Twig support) | Medium      | Medium      |
| F50 | [Twig filters and functions](todo/lsp-features.md#f50-twig-filters-and-functions) (blocks full Twig support) | Medium      | Medium      |
| F57 | ["Add translation" quick fix for an undeclared Symfony key](todo/lsp-features.md#f57-add-translation-quick-fix-for-an-undeclared-symfony-key) (depends on F29) | Medium      | Medium      |
| F58 | [Configured Twig roots and namespaces](todo/lsp-features.md#f58-configured-twig-roots-and-namespaces) | Low-Medium  | Medium      |
| F31 | [Symfony Twig templates outside `templates/`](todo/lsp-features.md#f31-symfony-twig-templates-outside-templates) | Low-Medium  | Medium      |
| F30 | [Symfony translations outside YAML, XLIFF, and the `trans` filter](todo/lsp-features.md#f30-symfony-translations-outside-yaml-xliff-and-the-trans-filter) | Low-Medium  | Medium      |
| F28 | [Symfony route names a loader composes](todo/lsp-features.md#f28-symfony-route-names-a-loader-composes) | Low-Medium  | Medium      |
| T54 | [Messenger handlers a service tag registers](todo/type-inference.md#t54-messenger-handlers-a-service-tag-registers) | Low-Medium  | Medium      |
| T55 | [Symfony array parameters are typed as plain `array`](todo/type-inference.md#t55-symfony-array-parameters-are-typed-as-plain-array) | Low         | Medium      |
| F37 | [Twig template variables](todo/lsp-features.md#f37-twig-template-variables) (blocks full Twig support) | Medium      | Medium-High |
| F34 | [Template names a PHP wrapper builds](todo/lsp-features.md#f34-template-names-a-php-wrapper-builds) | Medium      | Medium-High |
| F38 | [Twig components](todo/lsp-features.md#f38-twig-components) | Low-Medium  | Medium-High |
| F56 | [Live components](todo/lsp-features.md#f56-live-components) (depends on F38) | Low-Medium  | Medium-High |
| F44 | [Doctrine DQL and query builder strings](todo/lsp-features.md#f44-doctrine-dql-and-query-builder-strings) | Medium      | High        |
| F48 | [Symfony environment variables](todo/lsp-features.md#f48-symfony-environment-variables) | Medium      | Low-Medium  |
| F24 | [Doctrine lenses miss attribute mappings and generic repositories](todo/lsp-features.md#f24-doctrine-lenses-miss-attribute-mappings-and-generic-repositories) | Medium      | Medium      |
| F25 | [Names a PHP configurator spells as strings](todo/lsp-features.md#f25-names-a-php-configurator-spells-as-strings) | Medium      | Medium      |
| F45 | [Symfony events named by their class or a constant](todo/lsp-features.md#f45-symfony-events-named-by-their-class-or-a-constant) | Medium      | Medium      |
| F42 | [Symfony console arguments and options](todo/lsp-features.md#f42-symfony-console-arguments-and-options) | Medium      | Medium      |
| F39 | [Report unknown Symfony configuration keys](todo/lsp-features.md#f39-report-unknown-symfony-configuration-keys) | Medium      | Medium-High |
| F43 | [Doctrine entity fields in repository calls](todo/lsp-features.md#f43-doctrine-entity-fields-in-repository-calls) (depends on T45) | Medium      | Medium-High |
| F46 | [Symfony listener methods a tag or a default names](todo/lsp-features.md#f46-symfony-listener-methods-a-tag-or-a-default-names) | Low-Medium  | Low-Medium  |
| T45 | [Doctrine `getRepository(Entity::class)` is typed as the generic repository](todo/type-inference.md#t45-doctrine-getrepositoryentityclass-is-typed-as-the-generic-repository) | Low-Medium  | Medium      |
| T52 | [Symfony return types an argument decides](todo/type-inference.md#t52-symfony-return-types-an-argument-decides) | Low-Medium  | Medium      |
| D30 | [Getting a private Symfony service from the container is not reported](todo/diagnostics.md#d30-getting-a-private-symfony-service-from-the-container-is-not-reported) | Low-Medium  | Medium      |
| F26 | [Symfony service IDs a bundle or a definition API declares](todo/lsp-features.md#f26-symfony-service-ids-a-bundle-or-a-definition-api-declares) | Low-Medium  | Medium      |
| F40 | [Symfony configuration schemas the `TreeBuilder` walk misses](todo/lsp-features.md#f40-symfony-configuration-schemas-the-treebuilder-walk-misses) | Low-Medium  | Medium      |
| F41 | [Symfony forms and validation beyond a form type's own data class](todo/lsp-features.md#f41-symfony-forms-and-validation-beyond-a-form-types-own-data-class) | Low-Medium  | Medium      |
| F47 | [Symfony Messenger transports](todo/lsp-features.md#f47-symfony-messenger-transports) | Low-Medium  | Medium      |
| F49 | [Symfony security names](todo/lsp-features.md#f49-symfony-security-names) | Low-Medium  | Medium      |
| F54 | [Symfony form options, constraint options, and serializer groups](todo/lsp-features.md#f54-symfony-form-options-constraint-options-and-serializer-groups) | Low-Medium  | Medium      |
| D31 | [Doctrine mappings that disagree with the entity are not reported](todo/diagnostics.md#d31-doctrine-mappings-that-disagree-with-the-entity-are-not-reported) | Low-Medium  | Medium-High |
| D40 | [A Doctrine entity mapped in XML or YAML is not checked for uninitialized properties](todo/diagnostics.md#d40-a-doctrine-entity-mapped-in-xml-or-yaml-is-not-checked-for-uninitialized-properties) | Low         | Medium      |
| T56 | [A DBAL result's row count ignores the connection's driver](todo/type-inference.md#t56-a-dbal-results-row-count-ignores-the-connections-driver) | Low         | Low-Medium  |
| F51 | [Class names in Twig's `constant()` and `enum()`](todo/lsp-features.md#f51-class-names-in-twigs-constant-and-enum) | Low         | Low-Medium  |
| F52 | [Asset paths in Twig](todo/lsp-features.md#f52-asset-paths-in-twig) | Low         | Low-Medium  |
| T53 | [Doctrine collections narrowed by `isEmpty()`](todo/type-inference.md#t53-doctrine-collections-narrowed-by-isempty) | Low         | Medium      |
| F23 | [Rename a class through its XML entity-escaped occurrences](todo/lsp-features.md#f23-rename-a-class-through-its-xml-entity-escaped-occurrences) | Low         | Medium      |
| F53 | [Stimulus controllers in Twig](todo/lsp-features.md#f53-stimulus-controllers-in-twig) | Low         | Medium      |
| F55 | [Symfony ExpressionLanguage strings](todo/lsp-features.md#f55-symfony-expressionlanguage-strings) | Low         | Medium-High |

## WordPress

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| WP1 | [WordPress demo project and a real-world baseline](todo/wordpress.md#wp1-wordpress-demo-project-and-a-real-world-baseline) | High        | Low-Medium  |
| WP2 | [Audit what the WordPress stubs declare against the engine](todo/wordpress.md#wp2-audit-what-the-wordpress-stubs-declare-against-the-engine) | High        | Low         |
| WP4 | [Hook names](todo/wordpress.md#wp4-hook-names) | High        | Medium      |
| WP3 | [WordPress core for a plugin or theme opened on its own](todo/wordpress.md#wp3-wordpress-core-for-a-plugin-or-theme-opened-on-its-own) | Medium-High | Medium      |
| T57 | [A `global` variable is untyped unless a `@var` sits above it](todo/type-inference.md#t57-a-global-variable-is-untyped-unless-a-var-sits-above-it) | Medium      | Medium      |
| WP6 | [Template parts](todo/wordpress.md#wp6-template-parts) | Medium      | Medium      |
| E7  | [Stub-based framework patches](todo/external-stubs.md#e7-stub-based-framework-patches)                                                                                      | Medium      | Medium-High |
| WP7 | [Return types the WordPress stubs do not declare](todo/wordpress.md#wp7-return-types-the-wordpress-stubs-do-not-declare) (depends on WP2) | Medium      | Medium      |
| WP5 | [Hook callback parameter and return types](todo/wordpress.md#wp5-hook-callback-parameter-and-return-types) (depends on WP4) | Medium-High | Medium-High |
| WP8 | [Sage view composers](todo/wordpress.md#wp8-sage-view-composers) | Medium      | Medium      |

## Drupal

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| DR1 | [Drupal demo project and a real-world baseline](todo/drupal.md#dr1-drupal-demo-project-and-a-real-world-baseline) | High        | Low-Medium  |
| F59 | [Send Twig, YAML, and XML documents to the server from Zed](todo/lsp-features.md#f59-send-twig-yaml-and-xml-documents-to-the-server-from-zed) (Zed PR) | Medium | Low |
| DR2 | [Service IDs from `*.services.yml`](todo/drupal.md#dr2-service-ids-from-servicesyml) | High        | Medium      |
| DR5 | [Route names](todo/drupal.md#dr5-route-names) | Medium-High | Medium      |
| DR6 | [Hooks](todo/drupal.md#dr6-hooks) | Medium-High | Medium      |
| DR8 | [Permission and configuration names](todo/drupal.md#dr8-permission-and-configuration-names) | Low-Medium  | Medium      |
| DR3 | [Entity storage and entity type IDs](todo/drupal.md#dr3-entity-storage-and-entity-type-ids) | High        | Medium-High |
| DR7 | [Render arrays, theme hooks, and libraries](todo/drupal.md#dr7-render-arrays-theme-hooks-and-libraries) | Medium      | Medium-High |
| DR4 | [Entity field access](todo/drupal.md#dr4-entity-field-access) (depends on DR3) | Medium-High | High        |

## PHPUnit & Pest

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| TS4 | [Data provider declaration checks](todo/testing.md#ts4-data-provider-declaration-checks) | Low-Medium  | Low         |
| TS5 | [Assertion libraries](todo/testing.md#ts5-assertion-libraries) | Low-Medium  | Low         |
| TS6 | [`setUp()` and `tearDown()` that skip the parent](todo/testing.md#ts6-setup-and-teardown-that-skip-the-parent) | Low         | Low         |
| TS7 | [Prophecy](todo/testing.md#ts7-prophecy) | Low         | Low-Medium  |
| TS1 | [Mock method names](todo/testing.md#ts1-mock-method-names) | High        | Medium      |
| TS2 | [Go to the test, and back](todo/testing.md#ts2-go-to-the-test-and-back) | Medium      | Medium      |
| TS3 | [Create a test for a class](todo/testing.md#ts3-create-a-test-for-a-class) (depends on TS2) | Medium      | Medium-High |

## Laravel & Blade

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| L27 | [Legacy `Controller@method` action strings](todo/laravel.md#l27-legacy-controllermethod-action-strings)                                                                     | Low         | Low         |
| L60 | [Read relation callback overrides and fallbacks from the declared signature](todo/laravel.md#l60-read-relation-callback-overrides-and-fallbacks-from-the-declared-signature) | Low         | Low-Medium  |
| L29 | [Livewire and Volt component names](todo/laravel.md#l29-livewire-and-volt-component-names) (Livewire projects only)                                                          | Low         | Low-Medium  |
| L30 | [Eloquent attribute-array key completion](todo/laravel.md#l30-eloquent-attribute-array-key-completion)                                                                      | Medium      | Medium      |
| L56 | [Typed Laravel connection names](todo/laravel.md#l56-typed-laravel-connection-names)                                                                                        | Medium      | Medium      |
| L58 | [Laravel rate limiter names](todo/laravel.md#l58-laravel-rate-limiter-names)                                                                                                | Medium      | Medium      |
| L53 | [Collection key types from the column for `keyBy` / `groupBy` / `pluck`](todo/laravel.md#l53-collection-key-types-from-the-column-for-keyby-groupby-pluck)                   | Medium      | Medium      |
| L49 | [Unguarded Eloquent mass assignment diagnostic](todo/laravel.md#l49-unguarded-eloquent-mass-assignment-diagnostic)                                                          | Medium      | Medium      |
| L57 | [Laravel queue names](todo/laravel.md#l57-laravel-queue-names)                                                                                                              | Low-Medium  | Medium      |
| L59 | [Typed controller middleware names](todo/laravel.md#l59-typed-controller-middleware-names)                                                                                  | Low-Medium  | Medium      |
| L31 | [String-key rename, highlight, and semantic tokens](todo/laravel.md#l31-string-key-rename-highlight-and-semantic-tokens)                                                    | Low-Medium  | Medium      |
| L42 | [Morph alias completion in array positions](todo/laravel.md#l42-morph-alias-completion-in-array-positions)                                                                  | Low-Medium  | Medium      |
| L50 | ["Create route" quick-fix for an unresolved route name](todo/laravel.md#l50-create-route-quick-fix-for-an-unresolved-route-name)                                            | Low-Medium  | Medium      |
| L61 | [Resolve aliased facade imports during route discovery](todo/laravel.md#l61-resolve-aliased-facade-imports-during-route-discovery)                                          | Low-Medium  | Medium      |
| L10 | [`RedirectResponse::withX()` dynamic methods](todo/laravel.md#l10-redirectresponsewithx-dynamic-methods)                                                                  | Low         | Medium      |
| L39 | [Unused view and translation key detection](todo/laravel.md#l39-unused-view-and-translation-key-detection)                                                                  | Low         | Medium      |
| L46 | [`->can()` on a user model the receiver does not name](todo/laravel.md#l46-can-on-a-user-model-the-receiver-does-not-name)                                                  | Medium-High | Medium-High |
| L17 | [Additional string contexts without booting](todo/laravel.md#l17-additional-string-contexts-without-booting) (middleware, assets, validation, Inertia)                     | Medium      | Medium-High |
| E7  | [Stub-based framework patches](todo/external-stubs.md#e7-stub-based-framework-patches)                                                                                      | Medium      | Medium-High |
| L1  | [Facade completion](todo/laravel.md#l1-facade-completion-upstream-method-generator-improvement) (upstream `facade-documenter` PRs)                              | High        | High        |
| L8  | [`withSum`/`withAvg`/`withMin`/`withMax` aggregate properties](todo/laravel.md#l8-withsum-withavg-withmin-withmax-aggregate-properties)                                  | Low-Medium  | High        |
| L45 | [`*_count` properties are offered on every relationship](todo/laravel.md#l45-_count-properties-are-offered-on-every-relationship)                                           | Low-Medium  | High        |

## Type Inference

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| T41 | [A reading of the callee's body overrides a declared `@param-out`](todo/type-inference.md#t41-a-reading-of-the-callees-body-overrides-a-declared-param-out)            | Low-Medium  | Low         |
| T26 | [Class constants named as docblock types (`Foo::BAR`, `Foo::BAR_*`)](todo/type-inference.md#t26-class-constants-named-as-docblock-types-foobar-foobar_) | Medium      | Medium      |
| T49 | [A closure passed to a constructor gets no parameter types from it](todo/type-inference.md#t49-a-closure-passed-to-a-constructor-gets-no-parameter-types-from-it) | Medium      | Medium      |
| C13 | [`self::`/`static::` inside a `@require-extends` trait does not see the required class's static members](todo/completion.md#c13-selfstatic-inside-a-require-extends-trait-does-not-see-the-required-classs-static-members) | Low-Medium  | Medium      |
| D23 | [A rebound closure's scope is added to the lexical one rather than replacing it](todo/diagnostics.md#d23-a-rebound-closures-scope-is-added-to-the-lexical-one-rather-than-replacing-it) | Low-Medium  | Medium      |
| T43 | [`self::TypeAlias` inside `@extends`'s generic argument is not resolved](todo/type-inference.md#t43-selftypealias-inside-extendss-generic-argument-is-not-resolved) | Low         | Medium      |
| C5  | `#[ReturnTypeContract]` parameter-dependent return types                                                                                                                    | Low         | Medium      |
| T34 | [`static::CONST` over-narrows to the declaring class's value](todo/type-inference.md#t34-staticconst-over-narrows-to-the-declaring-classs-value)                            | Medium      | Medium-High |
| C7  | `class_alias()` support                                                                                                                                                     | Low-Medium  | Medium-High |
| D18 | [`array<int, T>` is accepted wherever a `list<T>` is declared](todo/diagnostics.md#d18-arrayint-t-is-accepted-wherever-a-listt-is-declared)                                 | Low         | Medium-High |
| T29 | [Definite vs possible variable existence tracking](todo/type-inference.md#t29-definite-vs-possible-variable-existence-tracking)                                             | Medium      | High        |
| T46 | [Template arguments are not inferred from how the object is used later](todo/type-inference.md#t46-template-arguments-are-not-inferred-from-how-the-object-is-used-later) | Medium      | High        |
| T47 | [A closure assigned to a variable gets no parameter types from where it is used](todo/type-inference.md#t47-a-closure-assigned-to-a-variable-gets-no-parameter-types-from-where-it-is-used) | Medium      | High        |
| C1  | Array functions needing new code paths                                                                                                                                      | Medium      | High        |
| C4  | Non-array functions with dynamic return types                                                                                                                               | Low         | High        |

## Diagnostics

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| D29 | [`namespace` and `declare` bodies break the reachability flow](todo/diagnostics.md#d29-namespace-and-declare-bodies-break-the-reachability-flow)                              | Low         | Low-Medium  |
| D19 | [`invalid_member_access` cannot tell a property read from a write](todo/diagnostics.md#d19-invalid_member_access-cannot-tell-a-property-read-from-a-write) (includes PHP 8.4 `private(set)`) | Medium      | Medium      |
| D16 | [`unreachable_match_arm` ignores literal subject types](todo/diagnostics.md#d16-unreachable_match_arm-ignores-literal-subject-types)                                        | Low-Medium  | Medium      |
| D33 | [Private properties nothing reads or writes are not reported](todo/diagnostics.md#d33-private-properties-nothing-reads-or-writes-are-not-reported) | Low-Medium  | Medium      |
| D39 | [Writes to a `@readonly` property are not checked](todo/diagnostics.md#d39-writes-to-a-readonly-property-are-not-checked) | Low         | Medium      |
| D20 | [`Foo::$bar` and `Foo::bar` are the same span](todo/diagnostics.md#d20-foobar-and-foobar-are-the-same-span)                                                                  | Low         | Medium      |
| D15 | [Unused parameter diagnostic](todo/diagnostics.md#d15-unused-parameter-diagnostic)                                                                                          | Low         | Medium      |
| D22 | [Member provenance is recomputed instead of recorded](todo/diagnostics.md#d22-member-provenance-is-recomputed-instead-of-recorded)                                        | Medium      | Medium-High |
| D17 | [`docblock_native_mismatch` only judges nullability](todo/diagnostics.md#d17-docblock_native_mismatch-only-judges-nullability)                                              | Low         | Medium-High |
| D21 | [A union of an unreachable and a missing member is reported by neither check](todo/diagnostics.md#d21-a-union-of-an-unreachable-and-a-missing-member-is-reported-by-neither-check) | Low         | Medium-High |

## Code Generation & Refactoring

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| A16 | [Snippet placeholder for extracted method name](todo/actions.md#a16-snippet-placeholder-for-extracted-method-name) (lets the user type over the generated name immediately) | Medium      | Medium      |
| A43 | [Update docblock generics](todo/actions.md#a43-update-docblock-generics)                                                                                                    | Medium      | Medium      |
| H13 | Declare missing property (native unknown-member diagnostic and PHPStan `property.notFound`)                                                                                 | Medium      | Medium      |
| H15 | Template bound from tip — add `@template T of X`                                                                                                                            | Medium      | Medium      |
| F17 | [Wire class move to `workspace/willRenameFiles`](todo/lsp-features.md#f17-wire-class-move-to-workspacewillrenamefiles)                                                       | Medium      | Medium      |
| A40 | [Generate method from call](todo/actions.md#a40-generate-method-from-call)                                                                                                  | High        | Medium-High |
| A41 | [Create class from non-existing name](todo/actions.md#a41-create-class-from-non-existing-name)                                                                              | High        | Medium-High |
| FX7 | [`add_return_type` — generate `@return` docblocks from function bodies](todo/fix-cli.md#fx7-add_return_type-generate-return-docblocks-from-function-bodies)                | Medium-High | Medium-High |
| H16 | `match.unhandled` — add missing match arms                                                                                                                                  | Medium      | Medium-High |
| F22 | [Merge a namespace onto one that shares a class name](todo/lsp-features.md#f22-merge-a-namespace-onto-one-that-shares-a-class-name)                                        | Low-Medium  | Medium-High |
| A34 | [Unified code action handler architecture](todo/actions.md#a34-unified-code-action-handler-architecture) (closure-based resolve, unified fix type)                          | Medium      | Very High   |

## Cleanup & Modernize

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| A28 | [Explicit nullable parameter type](todo/actions.md#a28-explicit-nullable-parameter-type-php-84-deprecation) (PHP 8.4 deprecation)                                           | Medium      | Low         |
| FX1 | [`deprecated` — replace deprecated symbol usage](todo/fix-cli.md#fx1-deprecated-replace-deprecated-symbol-usage)                                                           | Medium      | Low         |
| FX3 | [`phpstan.return.unusedType` — remove unused type from return union](todo/fix-cli.md#fx3-phpstanreturnunusedtype-remove-unused-type-from-return-union)                     | Medium      | Low         |
| H19 | `property.unused` / `method.unused` — remove unused member                                                                                                                  | Low         | Low         |
| H23 | `instanceof.alwaysTrue` — remove redundant instanceof check                                                                                                                 | Low         | Low         |
| H24 | `catch.neverThrown` — remove unnecessary catch clause                                                                                                                       | Low         | Low         |
| D5  | [External tool diagnostic suppression actions](todo/diagnostics.md#d5-external-tool-diagnostic-suppression-actions)                                                         | Low         | Low         |
| A25 | [`strpos` → `str_contains`](todo/actions.md#a25-strpos-str_contains-php-80) (PHP 8.0+)                                                                                     | Medium      | Medium      |
| H4  | `assign.byRefForeachExpr` — unset by-reference foreach variable                                                                                                             | Medium      | Medium      |
| FX2 | [`unused_variable` — remove unused variables](todo/fix-cli.md#fx2-unused_variable-remove-unused-variables)                                                                 | Medium      | Medium      |
| A29 | [Simplify boolean return](todo/actions.md#a29-simplify-boolean-return) (`if (cond) return true; return false;` → `return cond;`)                                            | Low-Medium  | Medium      |
| A45 | [Simplify with `?:`](todo/actions.md#a45-simplify-with-elvis-operator) (replace `$x ? $x : $y` with `$x ?: $y`)                                                             | Low-Medium  | Medium      |
| D28 | ["Remove unreachable code" is wired to PHPStan only](todo/diagnostics.md#d28-remove-unreachable-code-is-wired-to-phpstan-only)                                              | Low-Medium  | Medium      |
| H20 | `generics.callSiteVarianceRedundant` — remove redundant variance annotation                                                                                                 | Low         | Medium      |
| A31 | [Remove always-else](todo/actions.md#a31-remove-always-else-extract-guard-clause) (extract guard clause)                                                                    | Low-Medium  | Medium-High |
| A37 | [Simplify with `?->`](todo/actions.md#a37-simplify-with-nullsafe-operator) (replace null-checked chains with the nullsafe operator)                                       | Low-Medium  | Medium-High |
| A38 | [Convert if/elseif chain to `match`](todo/actions.md#a38-convert-ifelseif-chain-to-match)                                                                              | Low-Medium  | Medium-High |

## Editing Experience

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| F15 | [Go-to-declaration](todo/lsp-features.md#f15-go-to-declaration)                                                                                                              | Low-Medium  | Low         |
| F7  | [Evaluatable expression support (DAP integration)](todo/lsp-features.md#f7-evaluatable-expression-support-dap-integration)                                                  | Low-Medium  | Low         |
| F16 | [On-type `}` brace de-indent](todo/lsp-features.md#f16-on-type-brace-de-indent)                                                                                            | Low         | Low         |
| S2  | [Closure / arrow function parameter signature help](todo/signature-help.md#s2-closure-arrow-function-parameter-signature-help)                                             | Medium      | Medium      |
| A46 | [Honor `context.only` in code action responses](todo/actions.md#a46-honor-contextonly-in-code-action-responses)                                                             | Medium      | Medium      |
| N1  | Template engine (type-aware snippets)                                                                                                                                       | Medium      | Medium      |
| C8  | [Filesystem proximity as an affinity tiebreaker](todo/completion.md#c8-filesystem-proximity-as-an-affinity-tiebreaker)                                                      | Low-Medium  | Medium      |
| C3  | Go-to-definition for array shape keys via bracket access                                                                                                                    | Low-Medium  | Medium      |
| C6  | `#[ExpectedValues]` parameter value suggestions                                                                                                                             | Low         | Medium      |
| C10 | [Deprecation markers on class-name completions from all sources](todo/completion.md#c10-deprecation-markers-on-class-name-completions-from-all-sources)                     | Low         | Medium      |
| C12 | [The implicit `$value` of a `set` hook is not offered by variable completion](todo/completion.md#c12-the-implicit-value-of-a-set-hook-is-not-offered-by-variable-completion) | Low         | Medium      |
| S4  | Named argument awareness in active parameter                                                                                                                                | Low         | Medium      |
| S5  | Language construct signature help and hover                                                                                                                                 | Low         | Medium      |
| X7  | [Recency tracking](todo/indexing.md#x7-recency-tracking)                                                                                                                    | Medium      | Medium-High |
| S3  | Multiple overloaded signatures                                                                                                                                              | Low         | Medium-High |
| C11 | [Smarter member ordering after `->` / `::`](todo/completion.md#c11-smarter-member-ordering-after-)                                                                       | Medium      | High        |
| N2  | N-gram prediction from PHP corpus                                                                                                                                           | Medium      | Very High   |
| N3  | Fine-tuned GGUF sidecar model                                                                                                                                               | Medium      | Very High   |

## Editors & Distribution

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| F59 | [Send Twig, YAML, and XML documents to the server from Zed](todo/lsp-features.md#f59-send-twig-yaml-and-xml-documents-to-the-server-from-zed) (Zed PR) | Medium | Low |
| F14 | [Helix upstream PR](todo/lsp-features.md#f14-helix-upstream-pr)                                                                                                             | Low-Medium  | Low         |
| F11 | [Windows code signing](todo/lsp-features.md#f11-windows-code-signing)                                                                                                        | Medium      | Medium      |
| F12 | [IntelliJ / PHPStorm plugin](todo/lsp-features.md#f12-intellij-phpstorm-plugin)                                                                                            | High        | Medium-High |

## Indexing & Configuration

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| X16 | [Composer's own class lists bypass `[indexing] exclude`](todo/indexing.md#x16-composers-own-class-lists-bypass-indexing-exclude)                                            | Low-Medium  | Low         |
| X14 | [Forward Zed's `file_scan_exclusions` and `file_types`](todo/indexing.md#x14-forward-zeds-file_scan_exclusions-and-file_types) (waiting on [zed#66121](https://github.com/zed-industries/zed/pull/66121)) | Low         | Low         |
| X18 | [Watch `[indexing] include` and stub paths outside the workspace](todo/indexing.md#x18-watch-indexing-include-and-stub-paths-outside-the-workspace)                                         | Low-Medium  | Medium      |
| X13 | [Decide how workspace-wide edits treat excluded files](todo/indexing.md#x13-decide-how-workspace-wide-edits-treat-excluded-files)                                           | Low-Medium  | Medium      |
| X2  | Parallel file processing — remaining work                                                                                                                                   | Low-Medium  | Medium-High |
| X12 | [Say when an exclude hid the class a diagnostic names](todo/indexing.md#x12-say-when-an-exclude-hid-the-class-a-diagnostic-names)                                           | Low-Medium  | Medium-High |
