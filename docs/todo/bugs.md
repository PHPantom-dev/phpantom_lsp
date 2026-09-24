# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md); that table is also where
each bug's row lives in the current sprint/backlog.

Bugs land here from wherever they surface: found while working on another
task, or sweeps of the sample projects under `projects/`. Entries are
grouped by the mechanism that has to change, not by the symptom that
surfaced: one entry is one root cause, however many shapes it shows up in.

## Crashes

No outstanding items.

## Type comparison

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Arithmetic

No outstanding items.

## Symbol resolution

No outstanding items.

## Array types

No outstanding items.

## Docblock handling

No outstanding items.

## Laravel

## B339. A parent model's `$fillable` and `$casts` do not reach the child

**Impact: Medium · Complexity: Medium**

```php
class BaseModel extends Model { protected $fillable = ['uuid']; protected $casts = ['is_archived' => 'boolean']; }
class User extends BaseModel {}
```

`$user->uuid` and `$user->is_archived` are missing: `LaravelModelProvider`
(`src/virtual_members/laravel/mod.rs`) reads `class.laravel()` for the
model itself only. PHP inherits a property the child does not redeclare,
so each model-metadata field (`$fillable`, `$guarded`, `$casts`, `$dates`,
`$attributes`, `$hidden`, `$appends`, `$primaryKey`, `$keyType`,
`$timestamps`, the timestamp column constants) has to come from the
nearest class in the parent chain that declares it. That needs the
metadata to record "declared" separately from "empty".

**Test:** `laravel_eloquent_magic::columns_declared_on_a_parent_model_are_inherited`.

## B340. Column-name string completion recovers its receiver from text

**Impact: Medium · Complexity: Medium**

`$user->posts()->where('|')` and `$user->posts->where('|')` offer no
columns. `extract_subject_backwards` in `src/completion/eloquent_string.rs`
scans the source backwards for an identifier, which stops at `->` and `)`,
so the receiver is never a chain. This is a second, text-based resolver
beside the shared pipeline. Resolve the receiver expression through the
forward walker instead (`HasMany<Post, …>` mixes in `Builder<Post>`, and
the relation property is `Collection<int, Post>`), then read the model from
the resolved type.

**Tests:** `laravel_eloquent_magic::relationship_query_where_offers_the_related_models_columns`
and `relation_collection_where_offers_the_related_models_columns`.

## B341. Any `->name()` call in a route file registers a route

**Impact: Low · Complexity: Medium**

`$builder->name('x')->save()` or `$obj->getUser()->name('user')` in
`routes/web.php` registers a route called `x`/`user`, so a typo in a real
`route('x')` call is not flagged. `walk_expr` in
`src/virtual_members/laravel/route_names.rs` accepts a `->name()` on any
receiver. Requiring a registration verb in the chain is not enough on its
own, because router macros (`Route::inertia()`, `Route::livewire()`) name
routes too; the receiver has to be judged to be the router (the `Route`
facade, a `Router`, or `$this` inside a router macro).

**Test:** `laravel_route_names::a_name_call_on_an_unrelated_builder_is_not_a_route`.

## B342. Published package views under `resources/views/vendor` are ignored

**Impact: Low-Medium · Complexity: Medium**

`loadViewsFrom($path, 'widgets')` registers `resources/views/vendor/widgets`
*ahead of* the package's own directory, so a published copy is what renders
and a view only the published directory holds is still `widgets::name`.
Neither `scan_view_names` (`src/blade/discovery.rs`) nor
`resolve_view_definitions` (`src/virtual_members/laravel/view_names.rs`)
looks there.

**Tests:** `laravel_view_names::a_published_copy_of_a_package_view_wins_over_the_original`,
`a_view_only_the_published_directory_holds_is_known`, and
`a_published_package_template_is_typed_by_its_namespaced_call_site`.

## B343. View call-site data unions a key's values instead of keeping the last write

**Impact: Low · Complexity: Medium**

`view('x', ['user' => $a, 'user' => $b])` and
`view('x', ['user' => $a])->with('user', $b)` both leave the template with
only `$b` (a PHP array keeps the last duplicate key, and `View::with()`
overwrites), but call-site inference types `$user` as the union of both.
Within one call site the last write should win; the union is only right
across *different* call sites.

**Tests:** `laravel_view_names::a_duplicate_key_in_the_data_array_keeps_the_last_value`
and `a_with_call_replaces_the_same_key_from_the_data_argument`.

## B344. A config list value reads as an empty shape

**Impact: Low-Medium · Complexity: Low**

`'handlers' => [FooHandler::class, BarHandler::class]` makes
`config('logging.handlers')` hover as `array{}`: `array_node` in
`src/virtual_members/laravel/config_values.rs` skips every element without
a key. A keyless array is a list and should type as one (`list<…>` of its
element types, or at least a non-empty `array`).

**Test:** `laravel_config_values::a_list_value_is_not_an_empty_array`.

## B345. Config key enumeration reads a key's raw source instead of its value

**Impact: Low · Complexity: Low**

`'it\'s' => …` declares the key `it's`, but the unknown-config-key
diagnostic flags `config('app.it\'s')`/`config("app.it's")` because the key
reader (`src/virtual_members/laravel/config_keys.rs` and the enumeration
behind completion) slices the literal's source text. Read the literal's
unescaped `value`, as `config_values.rs` now does.

**Test:** `laravel_config_values::a_key_with_an_escaped_quote_is_named_by_its_value`.

## B346. Config defaults are merged recursively instead of Laravel's top-level merge

**Impact: Low-Medium · Complexity: Medium**

`mergeConfigFrom()` is `array_merge($package, $app)`: a group the
application publishes replaces the package's group whole. The framework's
own defaults (`LoadConfiguration`) merge one level deeper, and only for its
list of mergeable options. `merge_defaults` merges at every level and key
enumeration takes the union of all sources, so keys the application
deliberately removed are still offered and accepted.

**Tests:** `laravel_config_values::a_published_group_replaces_the_packages_group_whole`
and `an_application_group_replaces_the_framework_group_whole`.

## B347. Config go-to-definition never falls back to a package or framework file

**Impact: Low-Medium · Complexity: Medium**

When `config/<file>.php` exists, `resolve_config_key_declaration`
(`config_keys.rs`) returns line 0 of it even when the key is declared only
by the package's config (`mergeConfigFrom`) or the framework's
`vendor/laravel/framework/config/<file>.php`. The key is known (no
diagnostic, hover types it), so navigation should reach the file that
declares it.

**Tests:** `laravel_config_values::definition_of_an_unpublished_package_key_reaches_the_package_file`
and `definition_of_a_framework_default_key_reaches_the_framework_file`.

## B348. Translation groups in lang subdirectories are not enumerated

**Impact: Low-Medium · Complexity: Low**

Laravel's `FileLoader` reads `__('admin/users.title')` from
`lang/en/admin/users.php`. Go-to-definition reaches it, but the
unknown-key diagnostic and completion do not know the group:
`extract_lang_file_stem` (`src/completion/laravel_string_keys/enumerate.rs`)
keeps only the last path segment.

**Test:** `laravel_translation_keys::a_subdirectory_group_is_known`.

## B349. Package and published translation files are misclassified

**Impact: Medium · Complexity: Medium**

Two halves of the same path classification:

- A published override in `lang/vendor/<namespace>/<locale>/<group>.php`
  overrides and extends the package's own file, but the namespaced branch
  only walks the registered package directories, so the override is not a
  definition, hover quotes the package's line, and a key only the
  override adds is flagged.
- Any file under a `/lang/` directory counts as an application group
  (`is_lang_php_uri` in `enumerate.rs`, the `contains("/lang/")` check in
  `trans_keys.rs`), so a package's own `lang/en/messages.php` and a
  published `lang/vendor/…` file make `messages.x` a known *unnamespaced*
  key.

**Tests:** `laravel_translation_keys::a_published_override_is_a_definition_of_the_package_key`,
`a_published_override_is_the_line_hover_quotes`,
`a_key_only_the_published_override_adds_is_known`,
`a_published_package_file_is_not_an_application_group`,
`a_key_only_a_published_package_file_declares_is_unknown`, and
`a_package_translation_is_not_an_application_group`.

## B350. A `dirname(__DIR__)` path argument is not followed

**Impact: Low · Complexity: Low**

`$this->loadTranslationsFrom(dirname(__DIR__).'/lang', 'billing')` is the
common spelling in a package whose provider sits in `src/`, but
`extract_dir_concat_path` (`src/virtual_members/laravel/helpers.rs`) only
handles a bare `__DIR__` on the left of the concatenation, so the namespace
resolves nowhere. The same helper serves views, config, and routes.

**Test:** `laravel_translation_keys::a_namespace_registered_through_dirname_dir_resolves`.

## B351. Package-tools `hasTranslations()` registers nothing

**Impact: Low-Medium · Complexity: Medium**

A `spatie/laravel-package-tools` provider declares its resources through
`configurePackage(Package $package)`:
`$package->name('laravel-billing')->hasTranslations()` registers the
package's `resources/lang` under the short name `billing` (the name with
its `laravel-` prefix dropped). None of the `has*()` calls are modelled,
so every such package's translation keys (and, by the same route, its
views and config) are unknown.

**Test:** `laravel_translation_keys::a_package_tools_provider_registers_its_short_name`.

## B352. `@lang` and `@choice` arguments are not translation keys

**Impact: Low-Medium · Complexity: Low**

`translate_directive` (`src/blade/directives.rs`) lowers `@lang('x')` and
`@choice('x', $n)` to a generic `blade_directive(...)` call, so the key gets
no completion, go-to-definition, hover, or diagnostic. They compile to
`app('translator')->get()` / `->choice()` and should lower to
`__()`/`trans_choice()` so the translation string context sees them.

**Tests:** `laravel_translation_keys::the_lang_directive_reaches_the_line`
and `the_choice_directive_reaches_the_line`.

## B353. `.env.<environment>` files are not read

**Impact: Low · Complexity: Low**

Laravel loads `.env.{APP_ENV}` in place of `.env` when it exists, which is
how `.env.testing` supplies a test suite's database. `ENV_FILES` in
`src/virtual_members/laravel/env_vars.rs` lists only `.env` and
`.env.example`, so a variable declared only in `.env.testing` is reported
as undeclared and has no definition. Which environment is active is not
statically known, so every `.env.<name>` should count as a declaration.

**Test:** `laravel_env_keys::an_environment_specific_dotenv_file_declares_the_variable_too`.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
