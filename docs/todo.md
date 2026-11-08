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
| DR10 | [Plugin IDs](todo/drupal.md#dr10-plugin-ids) | Medium | Medium |
| DR26 | [Annotation class names](todo/drupal.md#dr26-annotation-class-names) (depends on DR10) | Low-Medium | Low-Medium |
| DR25 | [Warn on unknown plugin IDs](todo/drupal.md#dr25-warn-on-unknown-plugin-ids) (depends on DR10) | Medium | Low-Medium |
| DR17 | [Warn on unknown service IDs](todo/drupal.md#dr17-warn-on-unknown-service-ids) | Medium | Low-Medium |
| T70 | [A decorated service is typed as the class it decorates](todo/type-inference.md#t70-a-decorated-service-is-typed-as-the-class-it-decorates) | Low-Medium | Low |
| F68 | [Method names under `calls:` in a services file](todo/lsp-features.md#f68-method-names-under-calls-in-a-services-file) | Low | Low |
| Y20 | [`config-plugin` entries in `composer.json`](todo/yii.md#y20-config-plugin-entries-in-composerjson) | Low         | Low         |
| Y12 | [Missing layouts and view directories](todo/yii.md#y12-missing-layouts-and-view-directories) | Low-Medium  | Low-Medium  |
| Y16 | [Asset bundle file paths](todo/yii.md#y16-asset-bundle-file-paths) | Low         | Low-Medium  |
| Y19 | [Aliases in workspace symbols and folding](todo/yii.md#y19-aliases-in-workspace-symbols-and-folding) | Low         | Low-Medium  |
| Y11 | [Check what a DI definition passes, not only what it names](todo/yii.md#y11-check-what-a-di-definition-passes-not-only-what-it-names) | Medium      | Medium      |
| Y13 | [Translation message IDs](todo/yii.md#y13-translation-message-ids) | Medium      | Medium      |
| Y14 | [Event listeners in the events configuration](todo/yii.md#y14-event-listeners-in-the-events-configuration) | Medium      | Medium      |
| Y15 | [Property names in hydrator attributes and form fields](todo/yii.md#y15-property-names-in-hydrator-attributes-and-form-fields) | Low-Medium  | Medium      |
| Y17 | [Key paths in `yiisoft/arrays`](todo/yii.md#y17-key-paths-in-yiisoftarrays) | Low         | Medium      |
| Y7  | [Render parameters against the template's `@var` list](todo/yii.md#y7-render-parameters-against-the-templates-var-list) | Medium      | Medium-High |
| Y8  | [Column names in query arrays](todo/yii.md#y8-column-names-in-query-arrays) | Low-Medium  | High        |
| Y18 | [Active Record attributes and relations](todo/yii.md#y18-active-record-attributes-and-relations) (builds on Y8) | Low-Medium  | Medium-High |
| Y21 | [CSS class names in HTML helpers](todo/yii.md#y21-css-class-names-in-html-helpers) | Low         | Medium-High |
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
| F27 | [Symfony route parameters](todo/lsp-features.md#f27-symfony-route-parameters) (blocks full Symfony support) | Medium      | Medium      |
| F57 | ["Add translation" quick fix for an undeclared Symfony key](todo/lsp-features.md#f57-add-translation-quick-fix-for-an-undeclared-symfony-key) | Medium      | Medium      |
| F58 | [Configured Twig roots and namespaces](todo/lsp-features.md#f58-configured-twig-roots-and-namespaces) | Low-Medium  | Medium      |
| F31 | [Symfony Twig templates outside `templates/`](todo/lsp-features.md#f31-symfony-twig-templates-outside-templates) | Low-Medium  | Medium      |
| F66 | [Report Twig template diagnostics from `analyze`](todo/lsp-features.md#f66-report-twig-template-diagnostics-from-analyze) | Low-Medium  | Low-Medium  |
| F64 | [Report unknown parameters in YAML and XML configuration](todo/lsp-features.md#f64-report-unknown-parameters-in-yaml-and-xml-configuration) | Low-Medium  | Low-Medium  |
| F63 | [Check container parameters outside `app.*`](todo/lsp-features.md#f63-check-container-parameters-outside-app) | Low-Medium  | Medium      |
| T70 | [A decorated service is typed as the class it decorates](todo/type-inference.md#t70-a-decorated-service-is-typed-as-the-class-it-decorates) | Low-Medium | Low |
| F68 | [Method names under `calls:` in a services file](todo/lsp-features.md#f68-method-names-under-calls-in-a-services-file) | Low | Low |
| F62 | [Check Symfony logout route names](todo/lsp-features.md#f62-check-symfony-logout-route-names) | Low         | Low-Medium  |
| F65 | [Check interpolated and listed Twig names](todo/lsp-features.md#f65-check-interpolated-and-listed-twig-names) | Low         | Medium      |
| F30 | [Symfony translations outside YAML, XLIFF, and the `trans` filter](todo/lsp-features.md#f30-symfony-translations-outside-yaml-xliff-and-the-trans-filter) | Low-Medium  | Medium      |
| T54 | [Messenger handlers a service tag registers](todo/type-inference.md#t54-messenger-handlers-a-service-tag-registers) | Low-Medium  | Medium      |
| F61 | [Port TwigStan's tests](todo/lsp-features.md#f61-port-twigstans-tests) | Low-Medium | Medium |
| T55 | [Symfony array parameters are typed as plain `array`](todo/type-inference.md#t55-symfony-array-parameters-are-typed-as-plain-array) | Low         | Medium      |
| F34 | [Template names a PHP wrapper builds](todo/lsp-features.md#f34-template-names-a-php-wrapper-builds) | Medium      | Medium-High |
| F38 | [Twig components](todo/lsp-features.md#f38-twig-components) | Low-Medium  | Medium-High |
| F56 | [Live components](todo/lsp-features.md#f56-live-components) (depends on F38) | Low-Medium  | Medium-High |
| T68 | [A DQL field is typed by its property, not its column's type](todo/type-inference.md#t68-a-dql-field-is-typed-by-its-property-not-its-columns-type) | Low         | Medium      |
| T69 | [`IDENTITY()` in a DQL `SELECT` is `mixed`](todo/type-inference.md#t69-identity-in-a-dql-select-is-mixed) | Low         | Low-Medium  |
| T65 | [The expression builder's fragments are not literal strings](todo/type-inference.md#t65-the-expression-builders-fragments-are-not-literal-strings) | Low         | Low-Medium  |
| F48 | [Symfony environment variables](todo/lsp-features.md#f48-symfony-environment-variables) | Medium      | Low-Medium  |
| F24 | [Doctrine lenses miss attribute mappings and generic repositories](todo/lsp-features.md#f24-doctrine-lenses-miss-attribute-mappings-and-generic-repositories) | Medium      | Medium      |
| F25 | [Names a PHP configurator spells as strings](todo/lsp-features.md#f25-names-a-php-configurator-spells-as-strings) | Medium      | Medium      |
| F45 | [Symfony events named by their class or a constant](todo/lsp-features.md#f45-symfony-events-named-by-their-class-or-a-constant) | Medium      | Medium      |
| F42 | [Symfony console arguments and options](todo/lsp-features.md#f42-symfony-console-arguments-and-options) | Medium      | Medium      |
| F39 | [Report unknown Symfony configuration keys](todo/lsp-features.md#f39-report-unknown-symfony-configuration-keys) | Medium      | Medium-High |
| F46 | [Symfony listener methods a tag or a default names](todo/lsp-features.md#f46-symfony-listener-methods-a-tag-or-a-default-names) | Low-Medium  | Low-Medium  |
| T52 | [Symfony return types an argument decides](todo/type-inference.md#t52-symfony-return-types-an-argument-decides) | Low-Medium  | Medium      |
| D30 | [Getting a private Symfony service from the container is not reported](todo/diagnostics.md#d30-getting-a-private-symfony-service-from-the-container-is-not-reported) | Low-Medium  | Medium      |
| F40 | [Symfony configuration schemas the `TreeBuilder` walk misses](todo/lsp-features.md#f40-symfony-configuration-schemas-the-treebuilder-walk-misses) | Low-Medium  | Medium      |
| F41 | [Symfony forms and validation beyond a form type's own data class](todo/lsp-features.md#f41-symfony-forms-and-validation-beyond-a-form-types-own-data-class) | Low-Medium  | Medium      |
| F47 | [Symfony Messenger transports](todo/lsp-features.md#f47-symfony-messenger-transports) | Low-Medium  | Medium      |
| F49 | [Symfony security names](todo/lsp-features.md#f49-symfony-security-names) | Low-Medium  | Medium      |
| D31 | [Doctrine mappings that disagree with the entity are not reported](todo/diagnostics.md#d31-doctrine-mappings-that-disagree-with-the-entity-are-not-reported) | Low-Medium  | Medium-High |
| D40 | [A Doctrine entity mapped in XML or YAML is not checked for uninitialized properties](todo/diagnostics.md#d40-a-doctrine-entity-mapped-in-xml-or-yaml-is-not-checked-for-uninitialized-properties) | Low         | Medium      |
| T56 | [A DBAL result's row count ignores the connection's driver](todo/type-inference.md#t56-a-dbal-results-row-count-ignores-the-connections-driver) | Low         | Low         |
| F51 | [Class names in Twig's `constant()` and `enum()`](todo/lsp-features.md#f51-class-names-in-twigs-constant-and-enum) | Low         | Low-Medium  |
| F52 | [Asset paths in Twig](todo/lsp-features.md#f52-asset-paths-in-twig) | Low         | Low-Medium  |
| T53 | [Doctrine collections narrowed by `isEmpty()`](todo/type-inference.md#t53-doctrine-collections-narrowed-by-isempty) | Low         | Medium      |
| F23 | [Rename a class through its XML entity-escaped occurrences](todo/lsp-features.md#f23-rename-a-class-through-its-xml-entity-escaped-occurrences) | Low         | Medium      |
| F53 | [Stimulus controllers in Twig](todo/lsp-features.md#f53-stimulus-controllers-in-twig) | Low         | Medium      |
| F55 | [Symfony ExpressionLanguage strings](todo/lsp-features.md#f55-symfony-expressionlanguage-strings) | Low         | Medium-High |

## WordPress

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| WP9 | [The argument types the WordPress stubs declare](todo/wordpress.md#wp9-the-argument-types-the-wordpress-stubs-declare) | Medium      | Low-Medium  |
| WP3 | [WordPress core for a plugin or theme opened on its own](todo/wordpress.md#wp3-wordpress-core-for-a-plugin-or-theme-opened-on-its-own) | Medium-High | Medium      |
| WP15 | [`analyze` leaves out a plugin's files outside its PSR-4 roots](todo/wordpress.md#wp15-analyze-leaves-out-a-plugins-files-outside-its-psr-4-roots) | Medium      | Low         |
| WP6 | [Template parts](todo/wordpress.md#wp6-template-parts) | Medium      | Medium      |
| E7  | [Stub-based framework patches](todo/external-stubs.md#e7-stub-based-framework-patches)                                                                                      | Medium      | Medium-High |
| WP5 | [Hook callback parameter and return types](todo/wordpress.md#wp5-hook-callback-parameter-and-return-types) | Medium-High | Medium-High |
| WP8 | [Sage view composers](todo/wordpress.md#wp8-sage-view-composers) | Medium      | Medium      |
| WP10 | [Hook docblocks that do not match the call](todo/wordpress.md#wp10-hook-docblocks-that-do-not-match-the-call) (depends on WP5) | Low-Medium  | Medium      |
| WP12 | [Constants `wp-config.php` defines keep the value of one install](todo/wordpress.md#wp12-constants-wp-configphp-defines-keep-the-value-of-one-install) | Low         | Low-Medium  |
| WP11 | [`assertWPError()` and `assertNotWPError()` narrow](todo/wordpress.md#wp11-assertwperror-and-assertnotwperror-narrow) | Low         | Low         |
| WP14 | [`wp_parse_args()` of an object](todo/wordpress.md#wp14-wp_parse_args-of-an-object) | Low         | Medium      |

## Drupal

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| F59 | [Send Twig, YAML, and XML documents to the server from Zed](todo/lsp-features.md#f59-send-twig-yaml-and-xml-documents-to-the-server-from-zed) (Zed PR) | Medium | Low |
| T70 | [A decorated service is typed as the class it decorates](todo/type-inference.md#t70-a-decorated-service-is-typed-as-the-class-it-decorates) | Low-Medium | Low |
| F68 | [Method names under `calls:` in a services file](todo/lsp-features.md#f68-method-names-under-calls-in-a-services-file) | Low | Low |
| DR11 | [Form and render callbacks](todo/drupal.md#dr11-form-and-render-callbacks) | Low-Medium | Low-Medium |
| DR12 | [Translation placeholders](todo/drupal.md#dr12-translation-placeholders) | Low-Medium | Low-Medium |
| DR13 | [Module names and `*.info.yml`](todo/drupal.md#dr13-module-names-and-infoyml) | Low-Medium | Low-Medium |
| DR10 | [Plugin IDs](todo/drupal.md#dr10-plugin-ids) | Medium | Medium |
| DR14 | [Port PHPStan's Drupal extension tests](todo/drupal.md#dr14-port-phpstans-drupal-extension-tests) | Medium | Medium |
| DR4 | [Entity field access](todo/drupal.md#dr4-entity-field-access) | Medium-High | High        |
| DR16 | [Event subscribers](todo/drupal.md#dr16-event-subscribers) | Medium | Low-Medium |
| DR17 | [Warn on unknown service IDs](todo/drupal.md#dr17-warn-on-unknown-service-ids) | Medium | Low-Medium |
| DR18 | [Warn on unknown entity type IDs](todo/drupal.md#dr18-warn-on-unknown-entity-type-ids) | Medium | Low-Medium |
| DR19 | [Warn on unknown hook names](todo/drupal.md#dr19-warn-on-unknown-hook-names) | Low-Medium | Medium |
| DR20 | [Complete route names built in code](todo/drupal.md#dr20-complete-route-names-built-in-code) | Low-Medium | Medium |
| DR21 | [Return types core's docblocks leave wide](todo/drupal.md#dr21-return-types-cores-docblocks-leave-wide) | Medium | Low-Medium |
| DR22 | [Complete permissions built in code](todo/drupal.md#dr22-complete-permissions-built-in-code) | Low-Medium | Medium |
| DR23 | [Warn on unknown permission names](todo/drupal.md#dr23-warn-on-unknown-permission-names) | Medium | Low-Medium |
| DR24 | [Warn on unknown theme hook names](todo/drupal.md#dr24-warn-on-unknown-theme-hook-names) | Low-Medium | Medium-High |
| DR25 | [Warn on unknown plugin IDs](todo/drupal.md#dr25-warn-on-unknown-plugin-ids) | Medium | Low-Medium |
| DR26 | [Annotation class names](todo/drupal.md#dr26-annotation-class-names) | Low-Medium | Low-Medium |

## Yii

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| Y9  | [Views a template renders by name](todo/yii.md#y9-views-a-template-renders-by-name) | Medium      | Medium      |
| Y10 | [The renderer's configured view path](todo/yii.md#y10-the-renderers-configured-view-path) | Medium      | Medium      |
| Y11 | [Check what a DI definition passes, not only what it names](todo/yii.md#y11-check-what-a-di-definition-passes-not-only-what-it-names) | Medium      | Medium      |
| Y13 | [Translation message IDs](todo/yii.md#y13-translation-message-ids) | Medium      | Medium      |
| Y14 | [Event listeners in the events configuration](todo/yii.md#y14-event-listeners-in-the-events-configuration) | Medium      | Medium      |
| Y7  | [Render parameters against the template's `@var` list](todo/yii.md#y7-render-parameters-against-the-templates-var-list) | Medium      | Medium-High |
| Y12 | [Missing layouts and view directories](todo/yii.md#y12-missing-layouts-and-view-directories) | Low-Medium  | Low-Medium  |
| Y15 | [Property names in hydrator attributes and form fields](todo/yii.md#y15-property-names-in-hydrator-attributes-and-form-fields) | Low-Medium  | Medium      |
| Y8  | [Column names in query arrays](todo/yii.md#y8-column-names-in-query-arrays) | Low-Medium  | High        |
| Y18 | [Active Record attributes and relations](todo/yii.md#y18-active-record-attributes-and-relations) (builds on Y8) | Low-Medium  | Medium-High |
| Y20 | [`config-plugin` entries in `composer.json`](todo/yii.md#y20-config-plugin-entries-in-composerjson) | Low         | Low         |
| Y16 | [Asset bundle file paths](todo/yii.md#y16-asset-bundle-file-paths) | Low         | Low-Medium  |
| Y19 | [Aliases in workspace symbols and folding](todo/yii.md#y19-aliases-in-workspace-symbols-and-folding) | Low         | Low-Medium  |
| Y17 | [Key paths in `yiisoft/arrays`](todo/yii.md#y17-key-paths-in-yiisoftarrays) | Low         | Medium      |
| Y21 | [CSS class names in HTML helpers](todo/yii.md#y21-css-class-names-in-html-helpers) | Low         | Medium-High |

## PHPUnit & Pest

| #   | Item                                                                                                                                                            | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| TS5 | [Assertion libraries](todo/testing.md#ts5-assertion-libraries) | Low-Medium  | Low         |
| TS10 | [Benchmark for test code](todo/testing.md#ts10-benchmark-for-test-code) | Low-Medium | Low |
| TS8 | [Port PHPStan's PHPUnit and Mockery extension tests](todo/testing.md#ts8-port-phpstans-phpunit-and-mockery-extension-tests) | Medium | Low-Medium |
| TS9 | [PHPUnit and Pest demo project](todo/testing.md#ts9-phpunit-and-pest-demo-project) | Medium | Low-Medium |
| TS11 | [Mock unions in property docblocks](todo/testing.md#ts11-mock-unions-in-property-docblocks) | Medium | Low-Medium |
| TS7 | [Prophecy](todo/testing.md#ts7-prophecy) | Low         | Low-Medium  |

## Laravel & Blade

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| L68 | [Port Larastan's type and rule tests](todo/laravel.md#l68-port-larastans-type-and-rule-tests) | High | Medium |
| L1  | [Facade completion](todo/laravel.md#l1-facade-completion-upstream-method-generator-improvement) (upstream `facade-documenter` PRs)                              | High        | High        |
| L46 | [`->can()` on a user model the receiver does not name](todo/laravel.md#l46-can-on-a-user-model-the-receiver-does-not-name)                                                  | Medium-High | Medium-High |
| L69 | [Benchmarks for Laravel string names and Blade](todo/laravel.md#l69-benchmarks-for-laravel-string-names-and-blade) | Medium | Low-Medium |
| L49 | [Unguarded Eloquent mass assignment diagnostic](todo/laravel.md#l49-unguarded-eloquent-mass-assignment-diagnostic)                                                          | Medium      | Medium      |
| L56 | [Typed Laravel connection names](todo/laravel.md#l56-typed-laravel-connection-names)                                                                                        | Medium      | Medium      |
| L58 | [Laravel rate limiter names](todo/laravel.md#l58-laravel-rate-limiter-names)                                                                                                | Medium      | Medium      |
| BL20 | [Port Bladestan's tests](todo/blade.md#bl20-port-bladestans-tests) | Medium | Medium |
| E7  | [Stub-based framework patches](todo/external-stubs.md#e7-stub-based-framework-patches)                                                                                      | Medium      | Medium-High |
| L71 | [Inertia page props and asset helpers on a typed URL generator](todo/laravel.md#l71-inertia-page-props-and-asset-helpers-on-a-typed-url-generator) | Low | Low-Medium |
| L59 | [Typed controller middleware names](todo/laravel.md#l59-typed-controller-middleware-names)                                                                                  | Low-Medium  | Medium      |
| L31 | [String-key rename, highlight, and semantic tokens](todo/laravel.md#l31-string-key-rename-highlight-and-semantic-tokens)                                                    | Low-Medium  | Medium      |
| L50 | ["Create route" quick-fix for an unresolved route name](todo/laravel.md#l50-create-route-quick-fix-for-an-unresolved-route-name)                                            | Low-Medium  | Medium      |
| L61 | [Resolve aliased facade imports during route discovery](todo/laravel.md#l61-resolve-aliased-facade-imports-during-route-discovery)                                          | Low-Medium  | Medium      |
| L67 | [Laravel refactoring intentions](todo/laravel.md#l67-laravel-refactoring-intentions) | Low-Medium | Medium |
| L57 | [Laravel queue names](todo/laravel.md#l57-laravel-queue-names)                                                                                                              | Low-Medium  | Medium      |
| L8  | [`withSum`/`withAvg`/`withMin`/`withMax` aggregate properties](todo/laravel.md#l8-withsum-withavg-withmin-withmax-aggregate-properties)                                  | Low-Medium  | High        |
| L45 | [`*_count` properties are offered on every relationship](todo/laravel.md#l45-_count-properties-are-offered-on-every-relationship)                                           | Low-Medium  | High        |
| L27 | [Legacy `Controller@method` action strings](todo/laravel.md#l27-legacy-controllermethod-action-strings)                                                                     | Low         | Low         |
| L60 | [Read relation callback overrides and fallbacks from the declared signature](todo/laravel.md#l60-read-relation-callback-overrides-and-fallbacks-from-the-declared-signature) | Low         | Low-Medium  |
| L29 | [Livewire and Volt component names](todo/laravel.md#l29-livewire-and-volt-component-names) (Livewire projects only)                                                          | Low         | Low-Medium  |
| L10 | [`RedirectResponse::withX()` dynamic methods](todo/laravel.md#l10-redirectresponsewithx-dynamic-methods)                                                                  | Low         | Medium      |
| L39 | [Unused view and translation key detection](todo/laravel.md#l39-unused-view-and-translation-key-detection)                                                                  | Low         | Medium      |

## Type Inference

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| T41 | [A reading of the callee's body overrides a declared `@param-out`](todo/type-inference.md#t41-a-reading-of-the-callees-body-overrides-a-declared-param-out)            | Low-Medium  | Low         |
| T58 | [`new<T>` is not supported](todo/type-inference.md#t58-newt-is-not-supported) | Low-Medium  | Low-Medium  |
| T62 | [A sealed hierarchy is not narrowed by ruling out its other members](todo/type-inference.md#t62-a-sealed-hierarchy-is-not-narrowed-by-ruling-out-its-other-members) | Medium      | Medium      |
| T49 | [A closure passed to a constructor gets no parameter types from it](todo/type-inference.md#t49-a-closure-passed-to-a-constructor-gets-no-parameter-types-from-it) | Medium      | Medium      |
| T59 | [`T[K]` on a class template is not resolved](todo/type-inference.md#t59-tk-on-a-class-template-is-not-resolved) | Medium      | Medium      |
| T61 | [A regression that changes where a call resolves goes unnoticed](todo/type-inference.md#t61-a-regression-that-changes-where-a-call-resolves-goes-unnoticed) | Medium      | Medium      |
| C13 | [`self::`/`static::` inside a `@require-extends` trait does not see the required class's static members](todo/completion.md#c13-selfstatic-inside-a-require-extends-trait-does-not-see-the-required-classs-static-members) | Low-Medium  | Medium      |
| D23 | [A rebound closure's scope is added to the lexical one rather than replacing it](todo/diagnostics.md#d23-a-rebound-closures-scope-is-added-to-the-lexical-one-rather-than-replacing-it) | Low-Medium  | Medium      |
| T43 | [`self::TypeAlias` inside `@extends`'s generic argument is not resolved](todo/type-inference.md#t43-selftypealias-inside-extendss-generic-argument-is-not-resolved) | Low         | Medium      |
| T34 | [`static::CONST` over-narrows to the declaring class's value](todo/type-inference.md#t34-staticconst-over-narrows-to-the-declaring-classs-value)                            | Medium      | Medium-High |
| C7  | `class_alias()` support                                                                                                                                                     | Low-Medium  | Medium-High |
| T29 | [Definite vs possible variable existence tracking](todo/type-inference.md#t29-definite-vs-possible-variable-existence-tracking)                                             | Medium      | High        |
| T46 | [Template arguments are not inferred from how the object is used later](todo/type-inference.md#t46-template-arguments-are-not-inferred-from-how-the-object-is-used-later) | Medium      | High        |
| T47 | [A closure assigned to a variable gets no parameter types from where it is used](todo/type-inference.md#t47-a-closure-assigned-to-a-variable-gets-no-parameter-types-from-where-it-is-used) | Medium      | High        |
| C1  | Array functions needing new code paths                                                                                                                                      | Medium      | High        |

## Diagnostics

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| D29 | [`namespace` and `declare` bodies break the reachability flow](todo/diagnostics.md#d29-namespace-and-declare-bodies-break-the-reachability-flow)                              | Low         | Low-Medium  |
| D19 | [`inaccessible_member` cannot tell a property read from a write](todo/diagnostics.md#d19-inaccessible_member-cannot-tell-a-property-read-from-a-write) (includes PHP 8.4 `private(set)`) | Medium      | Medium      |
| D16 | [`unreachable_match_arm` ignores literal subject types](todo/diagnostics.md#d16-unreachable_match_arm-ignores-literal-subject-types)                                        | Low-Medium  | Medium      |
| D20 | [`Foo::$bar` and `Foo::bar` are the same span](todo/diagnostics.md#d20-foobar-and-foobar-are-the-same-span)                                                                  | Low         | Medium      |
| D22 | [Member provenance is recomputed instead of recorded](todo/diagnostics.md#d22-member-provenance-is-recomputed-instead-of-recorded)                                        | Medium      | Medium-High |
| D50 | [`unused-parameters` skips methods a subclass can override](todo/diagnostics.md#d50-unused-parameters-skips-methods-a-subclass-can-override) | Low         | Medium-High |
| D52 | [`incompatible_override` only checks a `static` return type](todo/diagnostics.md#d52-incompatible_override-only-checks-a-static-return-type) | Medium      | Medium-High |

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

## Cleanup & Modernize

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
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
| A46 | [Convert nested calls to the pipe operator](todo/actions.md#a46-convert-nested-calls-to-the-pipe-operator-php-85) (PHP 8.5+)                                              | Low-Medium  | Medium      |
| D28 | ["Remove unreachable code" is wired to PHPStan only](todo/diagnostics.md#d28-remove-unreachable-code-is-wired-to-phpstan-only)                                              | Low-Medium  | Medium      |
| A47 | [Convert a getter or setter method into a property hook](todo/actions.md#a47-convert-a-getter-or-setter-method-into-a-property-hook-php-84) (PHP 8.4+)                   | Low         | Medium      |
| A31 | [Remove always-else](todo/actions.md#a31-remove-always-else-extract-guard-clause) (extract guard clause)                                                                    | Low-Medium  | Medium-High |
| A37 | [Simplify with `?->`](todo/actions.md#a37-simplify-with-nullsafe-operator) (replace null-checked chains with the nullsafe operator)                                       | Low-Medium  | Medium-High |
| A38 | [Convert if/elseif chain to `match`](todo/actions.md#a38-convert-ifelseif-chain-to-match)                                                                              | Low-Medium  | Medium-High |

## Editing Experience

| #   | Item                                                                                                                                                                        | Impact      | Complexity  |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | ----------- |
| F7  | [Evaluatable expression support (DAP integration)](todo/lsp-features.md#f7-evaluatable-expression-support-dap-integration)                                                  | Low-Medium  | Low         |
| F16 | [On-type `}` brace de-indent](todo/lsp-features.md#f16-on-type-brace-de-indent)                                                                                            | Low         | Low         |
| S2  | [Closure / arrow function parameter signature help](todo/signature-help.md#s2-closure-arrow-function-parameter-signature-help)                                             | Medium      | Medium      |
| N1  | Template engine (type-aware snippets)                                                                                                                                       | Medium      | Medium      |
| C8  | [Filesystem proximity as an affinity tiebreaker](todo/completion.md#c8-filesystem-proximity-as-an-affinity-tiebreaker)                                                      | Low-Medium  | Medium      |
| C3  | Go-to-definition for array shape keys via bracket access                                                                                                                    | Low-Medium  | Medium      |
| C10 | [Deprecation markers on class-name completions from all sources](todo/completion.md#c10-deprecation-markers-on-class-name-completions-from-all-sources)                     | Low         | Medium      |
| C12 | [The implicit `$value` of a `set` hook is not offered by variable completion](todo/completion.md#c12-the-implicit-value-of-a-set-hook-is-not-offered-by-variable-completion) | Low         | Medium      |
| S4  | Named argument awareness in active parameter                                                                                                                                | Low         | Medium      |
| S5  | Language construct signature help and hover                                                                                                                                 | Low         | Medium      |
| X7  | [Recency tracking](todo/indexing.md#x7-recency-tracking)                                                                                                                    | Medium      | Medium-High |
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
| X14 | [Forward Zed's `file_scan_exclusions` and `file_types`](todo/indexing.md#x14-forward-zeds-file_scan_exclusions-and-file_types) (waiting on [zed#66121](https://github.com/zed-industries/zed/pull/66121)) | Low         | Low         |
| X18 | [Watch `[indexing] include` and stub paths outside the workspace](todo/indexing.md#x18-watch-indexing-include-and-stub-paths-outside-the-workspace)                                         | Low-Medium  | Medium      |
| X13 | [Decide how workspace-wide edits treat excluded files](todo/indexing.md#x13-decide-how-workspace-wide-edits-treat-excluded-files)                                           | Low-Medium  | Medium      |
| X2  | Parallel file processing — remaining work                                                                                                                                   | Low-Medium  | Medium-High |
| X12 | [Say when an exclude hid the class a diagnostic names](todo/indexing.md#x12-say-when-an-exclude-hid-the-class-a-diagnostic-names)                                           | Low-Medium  | Medium-High |
