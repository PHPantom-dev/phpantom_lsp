# PHPantom — Laravel

Known gaps and missing features in PHPantom's Laravel Eloquent support.
For the general architecture and virtual member provider design, see
`ARCHITECTURE.md`.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## Out of scope (and why)

| Item | Reason |
|------|--------|
| Container bindings registered dynamically or conditionally | Binding names or targets computed at runtime (variables, environment switches, loops) cannot be recovered statically. Literal `bind('name', Target::class)`-style registrations in app and package service providers are recovered, as are the `$bindings` / `$singletons` arrays. Names in the framework's own `registerCoreContainerAliases()` and the app's alias config already resolve via parsing. |
| Facade `getFacadeAccessor()` with string aliases | Requires booting the application. `@method static` tags provide a workable fallback. |
| Blade templates | Separate project. See `blade.md` for the implementation plan. |
| Model column types from a live database connection | Requires a reachable, migrated database plus credentials, and answers "true for that database" rather than "true for this code". Committed schema artifacts (migration files, `database/schema/*.sql` dumps) are now parsed statically (schema dump + migration scanning). |
| Legacy Laravel versions | We target the generic annotation style current Laravel and the PHPStan Laravel extensions use. Older code may degrade gracefully. |
| Application provider scanning | Low-value, high-complexity. |
| Macro discovery (`Macroable` trait) | Requires booting the application to inspect runtime `$macros` static property. `@method` tags provide a workable fallback. |
| Facade → concrete resolution via booting | Requires booting (`getFacadeRoot()`). When `getFacadeAccessor()` returns a `::class` reference, static resolution is possible without booting. See "Facade completion" section below. |
| Contract → concrete resolution | Fully out of scope, including core framework contracts. Calling a concrete-only method on a contract-typed value is unsound per the declared types — the diagnostic is intended, exactly as `fn (A $a) => $a->bMethod()` is not a false positive just because `B extends A` at every call site. Where the *framework's own* docblock is needlessly wide, fix the docblock upstream or via stub patches. |
| Manager → driver resolution | Requires instantiating the manager at runtime. |
| Narrowing a `MorphTo` relation to concrete models | `$comment->commentable` resolves to the generic `Illuminate\Database\Eloquent\Model`, which is what the relation declares. The morph map (now indexed, see L47/L42) is global rather than per-relation, so the only type it could supply is a union of *every* mapped model — a sound upper bound that is far wider than the truth and would report a concrete method as "not found on any of the N possible types". Annotate the relation with `@return MorphTo<Post\|Video, $this>` where the target set is actually known. |

---

## Philosophy (unchanged)

- **No application booting.** We never boot a Laravel application to
  resolve types.
- **Declared types are the contract.** A diagnostic is a false
  positive only if the code is correct *per the declared types* of
  what it calls. Code that works because of runtime container state
  (contract-typed values whose binding happens to be a specific
  concrete, string aliases from arbitrary providers) is flagged
  intentionally; the remedy is an assert, a narrower type in the
  code, or an upstream docblock fix — never a resolver assumption
  baked into PHPantom. When we do read framework/project facts, we
  *parse* the installed source and bail gracefully if the shape is
  unrecognized; we never ship version-specific baked-in lists.
- **Code-declared types win.** Column types declared in code (casts,
  accessors, `@property` tags, attribute defaults) are the most accurate
  source and always take precedence. Committed schema artifacts
  (migration files, `database/schema/*.sql` dumps) are now parsed
  statically via schema dump and migration scanning. A live database
  connection remains out of scope.
- **Generic relationship hints preferred.** We expect relationship methods
  to carry the generic `@return HasMany<Post, $this>` annotation that the
  PHPStan Laravel extensions read. Fallback heuristics are best-effort.
- **Facades rely on `@method` tags.** Laravel's facades ship with
  comprehensive `@method static` tags (2,047 across all facades as of
  Laravel 12.x) generated by a script. Our PHPDoc provider already
  parses these, and where a tag is too wide because the generator
  flattened it, a static call falls through to the class the facade's
  `getFacadeAccessor()` names. The residual gap is upstream: the
  generator throws away template parameters, generic arguments and
  conditional returns that the source method declares. See L1.

---

## L1. Facade completion — upstream `@method` generator improvement

Facades are the primary way Laravel developers interact with framework
services (`Cache::get(...)`, `DB::table(...)`, `Route::get(...)`, etc.).
The facade pattern works by forwarding static calls on the facade class
to instance methods on a concrete service class via `__callStatic()`.

Every facade already ships with comprehensive `@method static` tags
(2,047 tags across all facades in Laravel 12.x), generated by
Laravel's `facade-documenter` script.

### The problem: flattened signatures

The generator throws away everything the source method's docblock says
beyond a bare type name. The `Application` class has:

```php
/**
 * @template T of object
 * @param class-string<T> $abstract
 * @return T
 */
public function make($abstract) { ... }
```

But the `App` facade emits:

```php
@method static object|mixed make(string $abstract, array $parameters = [])
```

Four separate losses, all in `facade.php`:

- Template parameters (`T of object`) are resolved to their bound or to
  `mixed`.
- Generic arguments are dropped (`Collection<int, User>` →
  `Collection`).
- Conditional returns are unioned
  (`($id is int ? static : Collection)` → `static|Collection`).
- `class-string<T>` collapses to `string`, losing the binding that
  connects the argument to the return.

Every consumer of the tags — us, PHPStan, Psalm, PhpStorm — is left
with a signature far wider than the truth.

### Why this has to be fixed upstream

Resolving facades ourselves by parsing `getFacadeAccessor()` covers
only part of the problem:

1. **Half the facades use string aliases.** `Auth` returns `'auth'`,
   `Cache` returns `'cache'`, `DB` returns `'db'`. We resolve the ones
   the framework's own `registerCoreContainerAliases()` and the app's
   config declare; a binding registered at runtime by an arbitrary
   provider stays unresolvable without booting.
2. **The `@see` annotations are informal.** Facades list multiple
   `@see` classes (e.g. `Auth` references both `AuthManager` and
   `SessionGuard`), and the relationship between them isn't formal
   (one may be a `@mixin` of the other). We'd be reverse-engineering
   an undocumented script's output logic.
3. **Only PHPantom would benefit.** Any resolution technique we build
   is specific to our tool. Other IDEs and LSPs face the same problem.
4. **Manager pattern complicates things.** Many facades delegate to a
   "manager" class (`AuthManager`, `CacheManager`) which in turn
   delegates to a "driver" (`SessionGuard`, `FileStore`). The
   `@method` tags are already a flattened merge of both levels. We'd
   need to replicate that merging logic.

### Upstream generator changes

**PR target:** `laravel/facade-documenter`
(https://github.com/laravel/facade-documenter). The entire generator
is a single file (`facade.php`, ~850 lines) that uses PHPStan's own
phpDoc parser. The functions that flatten:

- `resolveDocblockTypes()` — the `GenericTypeNode` branch strips
  generic arguments entirely. Needs to preserve args in the output
  string.
- `handleUnknownIdentifierType()` — when encountering a template
  parameter like `T`, resolves it to its bound (`T of object` →
  `object`) or `mixed`. Needs to instead emit `T` as-is and add a
  `<T of object>` clause after the method name in the `@method` tag.
- `ConditionalTypeForParameterNode` handling — unions the if/else
  branches. Needs to preserve the conditional syntax verbatim.
- The `class-string` case in `IdentifierTypeNode` handling — maps
  `class-string` to `string`. Needs to preserve `class-string<T>`.

### Tooling support for the richer syntax

Laravel will not emit tags that break the tools their users run, so
the four changes have very different odds. Support as researched:

| Change | PHPStan | Psalm | PhpStorm | Intelephense | PHPantom |
|---|---|---|---|---|---|
| Generic args (`Collection<int, User>`) | yes | yes | yes, since 2022.1 ([WI-62267](https://youtrack.jetbrains.com/issue/WI-62267), Verified) | unknown | yes |
| Conditional return (`($id is int ? A : B)`) | yes | yes | unknown | yes, since 1.15.0 | yes |
| `class-string<T>` preserved | yes | yes | needs the template below | unknown | yes |
| Inline `<T of object>` after the method name | yes | **no** | **no** | unknown | yes |

The last row is the blocker. PhpStorm reports a false-positive
`Undefined class 'T'` inspection for a template referenced from a
`@method` tag ([WI-64921](https://youtrack.jetbrains.com/issue/WI-64921),
Open since 2022; the inference half is
[WI-72659](https://youtrack.jetbrains.com/issue/WI-72659), In Progress).
Psalm parses each `@method` entry with its own `ParseTreeCreator` and
raises `InvalidDocblock` when the entry does not come out as a method
tree; its method-name regex requires the name to be followed directly
by `(`, so `make<T of object>(` is not recognised as a method at all
(`Internal/PhpVisitor/Reflector/ClassLikeDocblockParser.php`). Neither
degrades gracefully.

So split the upstream work:

1. **Generic arguments, conditional returns, `class-string` without a
   template arg.** No known breakage anywhere, immediate benefit in
   four tools. Submit these first, as separate PRs so one contentious
   change cannot stall the rest.
2. **Inline templates.** Hold until PhpStorm ships WI-64921/WI-72659,
   or propose it as a `@phpstan-method` line that coexists with the
   plain `@method` so tools that choke keep reading the flattened one.

**Impact: High · Complexity: High (upstream PRs)**

---

## Model property source gaps

The `LaravelModelProvider` synthesizes virtual properties from several
sources on Eloquent models. The table below summarises what we handle
today and what is still missing.

### What we cover

| Source | Type info | Notes |
|--------|-----------|-------|
| `$casts` / `casts()` | Rich (built-in map, custom cast `get()` return type, enum, `Castable`, `CastsAttributes<TGet>` generics fallback) | |
| `$attributes` defaults | Literal type inference (string, bool, int, float, null, array) | Fallback when no `$casts` entry |
| `$fillable`, `$guarded`, `$hidden`, `$visible` | `mixed` | Last-resort column name fallback |
| Legacy accessors (`getXAttribute()`) | Method's return type | |
| Modern accessors (returns `Attribute`) | First generic arg of `Attribute<TGet>`, or `mixed` when unparameterised | |
| Relationship methods | Generic params or body inference | |
| Relationship `*_count` properties | `int` | `{snake_name}_count` for each relationship method |

### Gaps (ranked by impact ÷ effort)

---

#### L53. Collection key types from the column for `keyBy` / `groupBy` / `pluck`

**Impact: Medium · Complexity: Medium**

`keyBy` and `groupBy` produce a collection keyed by the column they were
given, but we type the key `array-key`
(`virtual_members/laravel/higher_order_proxy.rs`, `plan_result`). The
column is a literal in the overwhelming majority of calls and the value
type is already resolved, so the key is recoverable by the same model
property lookup `model-property<Model>` validation uses:

```php
$users->keyBy('email');   // want Collection<string, User>, get Collection<array-key, User>
$users->groupBy('id');    // want Collection<int, Collection<int, User>>
$posts->keyBy('user.id'); // dotted path, resolved segment by segment
```

`pluck` is the same lookup applied to the value rather than the key, and
the higher-order proxy forms (`$users->keyBy->email`) must agree with the
argument forms or the two spellings of one expression disagree.

The framework's own annotations widen these to `array-key`, so there is no
docblock to read — but `calebdw/phpstan-laravel` resolves all three, which
means a project running it currently gets a more precise answer from its
analyser than from its editor. Worth matching, including `groupBy`'s
array-argument nesting (one level per grouper) and `preserveKeys` affecting
only the innermost collection.

**Where to change:** `plan_result` in
`virtual_members/laravel/higher_order_proxy.rs` for the proxy forms, and
the collection return-type patches for the argument forms. The property
lookup already exists for `model-property<Model>`; the work is threading
a resolved key type through the generic substitution.

#### L54. Audit custom-builder and relation-closure inference against the PHPStan extensions

**Impact: Medium · Complexity: Medium-High**

Two areas where the PHPStan Laravel extensions have moved past what we
mirror, and where we have machinery that has not been checked against
them:

- **A custom builder surviving the chain.** We read
  `newEloquentBuilder()` (`virtual_members/laravel/model_extraction.rs`)
  and inject the builder, but it is not established that
  `Team::query()->where(…)->orderBy(…)` stays on `TeamBuilder` rather than
  degrading to `Builder<Team>` at the first inherited call, nor that
  static calls on the model and instance calls on the builder agree about
  what comes back.
- **Relation-constraint closure parameters.** We type closures for the
  `whereHas` family (`type_engine/variable/closure_resolution.rs`,
  `forward_walk/callable_inference.rs`). Unverified: dotted relation paths
  (`whereHas('stocks.warehouse', …)` should type the closure for
  `Warehouse`'s builder, resolving each segment against the model the
  previous one named), the `*Morph` variants' union of candidate builders
  plus their `$type` parameter, `withWhereHas` receiving both a builder
  and the relation, and closures in non-leading argument positions
  (`has('stocks', '>=', 1, 'and', fn ($q) => …)`).

**Where to change:** Write the assertion cases first — the existing
`tests/integration/completion_laravel.rs` conventions cover both areas —
and file what actually fails. Splitting this into concrete items once the
gaps are known is preferable to a broad rewrite of either subsystem.

#### L45. `*_count` properties are offered on every relationship

**Impact: Low-Medium · Complexity: High**

For each relationship method we synthesize a `{snake_name}_count`
property typed `int`, unconditionally. At runtime the attribute only
exists when the query that produced the model called `withCount()` /
`loadCount()`, or when the model lists the relation in `$withCount`.
So `$post->comments_count` completes and hovers cleanly even where it
is guaranteed to be `null`, and a user reading the completion list
cannot tell which of the two `comments`-derived entries is real.

The trade-off is deliberate: `withCount()` is common enough that
dropping the properties entirely would cost more than it gains, and
the alternative needs call-site tracking we do not have. Doing this
properly means threading "which counts were eager-loaded" from the
builder chain (`Post::withCount('comments')->first()`) into the model
type, which is the same machinery L8 needs for `withSum()` and
friends. `$withCount` on the model is the easy half and could be read
declaratively like `$casts` is today.

Until then the properties stay, so this is a precision gap rather
than a missing feature. Reported as a follow-up on
[#312](https://github.com/PHPantom-dev/phpantom_lsp/issues/312).

#### L8. `withSum()` / `withAvg()` / `withMin()` / `withMax()` aggregate properties

**Impact: Low-Medium · Complexity: High**

Less common than `withCount`; only affects codebases using aggregate
eager-loading. Cannot be inferred declaratively from the model alone;
requires tracking call-site string arguments.

Similar to `withCount`, these aggregate methods produce virtual
properties named `{relation}_{function}` (e.g.
`Order::withSum('items', 'price')` → `$order->items_sum`). The same
call-site tracking challenge applies, and the type depends on the
aggregate function (`withSum`/`withAvg` → `float`,
`withMin`/`withMax` → `mixed`).

The `@property` workaround applies here too.

#### L10. `View::withX()` and `RedirectResponse::withX()` dynamic methods

**Impact: Low · Complexity: Medium**

Most code uses `->with('key', $value)` instead of the dynamic
`->withKey($value)` form. Explicitly declared methods (`withErrors`,
`withInput`, etc.) already work.

Both `Illuminate\View\View` and `Illuminate\Http\RedirectResponse`
support dynamic `with*()` calls via `__call()`.  For example,
`view('home')->withUser($user)` is equivalent to
`->with('user', $user)`.

```php
view('home')->withUser($user);         // dynamic, no @method annotation
redirect('/')->withErrors($errors);    // has explicit withErrors(), but withFoo() is dynamic
```

The framework provides no `@method` annotations for arbitrary
`with*` calls — only specific ones like `withErrors()`,
`withInput()`, `withCookies()` etc. are declared as real methods.
Larastan handles the dynamic case in
`ViewWithMethodsClassReflectionExtension` and
`RedirectResponseMethodsClassReflectionExtension`, which treat any
`with*` call as valid and returning `$this`.

**Where to change:** This could be handled with a lightweight
virtual member provider that detects classes with a `__call` method
whose body checks `str_starts_with($method, 'with')`, or by
hard-coding the two known classes.  A simpler approach: add
`@method` tags to bundled stubs for the most common dynamic `with*`
methods, or document this as a known limitation.

---

#### L12. `HasUuids` / `HasUlids` trait — `$id` typed as `string`

**Impact: Low-Medium · Complexity: Medium**

Models that use `Illuminate\Database\Eloquent\Concerns\HasUuids` or
`HasUlids` have their primary key (`$id` by default) typed as
`string` instead of `int`. Currently PHPantom does not inspect these
traits, so `$model->id` resolves to `int` (from the default Model
stub) instead of `string`.

Larastan's `bug-2188.php` tests this: `assertType('string', $uuidModel->id)`.

**Where to change:** In `LaravelModelProvider::provide`, after
synthesizing other virtual properties, check whether the model's
`used_traits` (recursively, including parent traits) contains
`HasUuids` or `HasUlids`. If so, synthesize a virtual `id` property
typed as `string` (or override the existing one). The trait also
overrides `getKeyType()` to return `'string'` and
`getIncrementing()` to return `false`, but for virtual property
purposes just the `id` type is the main gap.

Alternatively, if the stubs for these traits include `@property`
tags or a typed `$id` override, the PHPDoc provider may handle it
automatically once the traits are loaded.

#### L47. Morph aliases in `*_type` column comparisons

**Impact: Low-Medium · Complexity: Medium-High**

The morph-map index (`virtual_members/laravel/morph_map.rs`) recognizes
alias strings in the positions Eloquent resolves through the map by
name: the `morphMap()` keys themselves, `Relation::getMorphedModel()`,
`Model::getActualClassNameForMorph()`, and the `$types` argument of the
`whereHasMorph()` family. It does **not** recognize an alias compared
against a morph type *column*, which is how much real code reads it:

```php
$query->where('commentable_type', 'post');
if ($comment->commentable_type === 'post') { … }
```

Both are alias literals, but recognizing them means knowing that the
column named on the other side is a polymorphic type column. The
information is available: a `morphTo()` relation declares its type
column (defaulting to `<relation>_type`), and the relation methods of
the model being queried are already parsed. The work is to collect the
morph type columns of a model, then match a string literal that appears
opposite one in a `where()` / comparison against the alias index.

Once a literal is recognized it inherits hover, go-to-definition,
find-references, and the enforced-map diagnostic for free, since those
dispatch on the `LaravelStringKind::MorphAlias` span kind.

#### L42. Morph alias completion in array positions

**Impact: Low-Medium · Complexity: Medium**

Morph aliases complete inside `Relation::getMorphedModel('|')` and
`Model::getActualClassNameForMorph('|')`, but not in the two array
positions where they are most often written:

```php
Relation::morphMap(['|' => Post::class]);
$query->whereHasMorph('commentable', ['|']);
```

The blocker is not the alias index but
`detect_laravel_string_key_context` in
`completion/laravel_string_keys.rs`, which recognizes only a string
that is the *first* argument of a call — it scans backwards from the
cursor and requires `(` immediately before the opening quote. Neither
an array key nor an element of a later argument matches that shape.

**Where to change:** teach the detector to walk back past an enclosing
`[` (and the argument commas before it) so it can report both the call
being made and which argument position and array slot the cursor sits
in. Every string kind benefits: the same limitation is why a config key
inside `Config::set(['a.b' => …])` does not complete either.

---

## Model columns from committed schema artifacts

Our column inference from code (casts, accessors, `@property`,
attribute defaults, fillable) is complete and precise — on annotated
codebases we beat every snapshot-based tool on accuracy and freshness.
But on a project with bare models (no casts beyond dates, no
docblocks), we draw a blank where other tools can still list columns.
The two items below close that gap using files already committed to
the repository, consistent with the philosophy above: **schema-derived
columns are a fallback**, slotted below every code-declared source in
the model provider's priority order, and they must never override a
type declared in code.

Shared infrastructure for both: a per-project table model
(`table name → ordered column list`, each column carrying a PHP type,
nullability, and the declaration site for go-to-definition). Table
names resolve from the model's `$table` property or the conventional
snake-case plural of the class name. The column set feeds the existing
virtual-property synthesis, `where{Column}` generation, attribute-array
key completion (L30), and column-string completion — one new source,
every existing consumer benefits.

---

## Laravel string-context intelligence (completion, hover, diagnostics)

This is the set of features that live *inside string literals* passed to
Laravel helpers and facades: `route('...')`, `config('...')`, `env('...')`,
`__('...')`, `view('...')`, and so on. The official Laravel VS Code
extension (https://github.com/laravel/vs-code-extension) is built almost
entirely around this layer, and PhpStorm + Laravel Idea cover it too.

### Why we do this statically (and they boot the app)

The PHPStan Laravel extensions gather these facts by **booting the user's
application** in the background and introspecting the live container
(`app('router')->getRoutes()`, `config()->all()`, `app()->getBindings()`,
etc.). The Laravel LSP does the same through `php artisan tinker
--execute`: 14 of its 18 data providers (routes, configs, translations,
views, models, middleware, auth, bindings, Blade components, paths, …)
run injected PHP inside the booted app; only env vars, public assets,
the Mix manifest, and controller names (regex-scraped) are gathered
statically. Its Eloquent column completion even shells out to
`artisan model:show --json` per model, which requires a **reachable,
migrated database**. PhpStorm's Laravel Idea and
`barryvdh/laravel-ide-helper` do the same thing differently: they
generate a snapshot (JSON payload or PHP stubs) that the IDE then reads.

All three share two problems:

1. **A snapshot is one environment on one code path.** The booted values
   reflect whatever bindings, config, and routes are registered under the
   default boot. A controller that rebinds a service, a config value that
   differs in production, or a route registered behind a feature flag is
   either invisible or actively wrong. The answer is "true for that boot,"
   not "true here."
2. **Staleness.** A snapshot is only as fresh as its last regeneration
   (manual for ide-helper, periodic background re-boot for the extension).
   Static scanning with a file watcher updates the instant the source file
   changes, not at the next heartbeat.
3. **Silent total failure.** When the app cannot boot (broken provider,
   missing `.env`, no database), the Laravel LSP's tinker call exits
   nonzero and every boot-dependent feature silently returns empty:
   completions, hovers, and diagnostics all vanish with no indication
   why. A static scanner keeps working on a project that doesn't boot —
   which is exactly when the developer needs tooling most.

Our position (consistent with the **No application booting** philosophy
above): the overwhelming majority of these facts live in static files we
can parse and keep fresh for free — `routes/*.php` (`->name()`),
`config/*.php` return arrays, `lang/**` PHP + JSON, `.env`, and
`resources/views/**`. We recover ~90% of the value with no code execution,
no stale snapshot, and correct behaviour on a project that doesn't even
boot. The dynamic tail (routes/config/bindings registered at runtime by a
closure) is the only part a snapshot wins on, and that is exactly the code
that is hard to reason about anyway.

### What already works

Go-to-definition is implemented for **route names** (`route()`), **config
keys**, **env vars**, **translation keys**, and **view names** via the
declaration scanners in `virtual_members/laravel/{route_names, config_keys,
env_vars, trans_keys, view_names}.rs`. These walk the relevant source files
and resolve a string key to its declaration site. `LaravelStringKey` also
flows through find-references, rename, and document-highlight. **Eloquent
relation and column name strings** have completion (`completion/eloquent_string.rs`).

The scanners already enumerate every valid key for go-to-definition. The
items below mostly wire that existing enumeration into three more LSP
endpoints rather than building new analysis.

#### L17. Additional string contexts without booting

**Impact: Medium · Complexity: Medium-High**

Constructs the extension and the Laravel LSP cover that we don't, and
that are statically recoverable (no booting):

- **Middleware aliases** — completion/go-to/diagnostics/hover for
  `->middleware('auth')`, `->withoutMiddleware(...)`, and the
  `#[Middleware]` controller attribute (string and array arguments).
  Aliases are declared statically in `bootstrap/app.php`
  (`$middleware->alias([...])`) on Laravel 11+, or the HTTP Kernel's
  `$middlewareAliases`/`$routeMiddleware` on older versions; the
  framework's built-in aliases and default groups live in
  `Illuminate\Foundation\Configuration\Middleware` and the Kernel base
  class, parseable from vendor source the same way the container alias
  table already is. Be **parameter-aware**: `auth:web` and
  `throttle:60,1` validate the alias before the `:`, and hover can show
  the middleware's `handle()` parameter signature. For groups (`'web'`,
  `'api'`), hover lists the member middleware.
- **Asset paths** — `asset('...')` (and `UrlGenerator::asset()`) against
  a filesystem walk of `public/`. Two cheap extensions the Laravel LSP
  has or lacks: `mix('...')` against `public/mix-manifest.json` (static
  JSON, legacy projects), and — which they do *not* support — Vite
  entry-point strings (`Vite::asset('resources/js/app.js')`, `@vite`
  arguments) checked against source-file existence.
- **Validation rules** — completion for the rule strings in
  `'field' => 'required|email'`. Rather than hardcoding a rule list
  (the Laravel LSP ships ~90 rules as LSP snippets, e.g.
  `between:${min},${max}`), derive it from the installed framework:
  the `Validator` concern declares one `validate{Rule}` method per
  rule, so scanning that vendor class yields the exact rule set for
  the project's Laravel version, plus whether each rule takes
  parameters. Custom rule classes (implementing `ValidationRule`) and
  literal `Validator::extend('name', …)` registrations extend the set.
  Trigger contexts to match: `$request->validate()` arg 0,
  `Validator::make()` arg 1, `$validator->sometimes()` arg 1, and array
  *values* (never keys — those are field names) inside the `rules()`
  method of `FormRequest` / `Livewire\Form` subclasses.
  **Rule parameters are references too:** `exists:users,email` and
  `unique:users` name a table and column (resolve to the model/column
  via the schema/migration table map or the model's own column sources);
  `same:password`, `different:`, `required_with:`, `gt/gte/lt/lte:`,
  `after:`/`before:` name *other fields* in the same rules array
  (complete and navigate to the sibling key); `in:`/`Rule::in()` values
  on an enum-cast field can complete from the enum's cases.
- **Inertia page paths** — `Inertia::render('...')` and
  `Inertia::modal()` arg 0, `Route::inertia()` arg 1, and the
  `inertia()` helper, against a filesystem walk of `resources/js/Pages`
  (page paths/extensions from `config/inertia.php`, read statically;
  default `resources/js/Pages` + `.vue`). Only relevant to Inertia
  projects. Pair the unknown-page diagnostic with a "create page" quick
  fix (the Laravel LSP scaffolds a `<script setup>/<template>` stub).
  Its second-argument prop-key completion (regex-parsing `defineProps`
  out of the `.vue` page) is shallow and optional — defer unless asked.

**Explicitly still out of scope** (see the table at the top): container
binding names (`app('...')`), facade string aliases, and anything else that
requires the live container. These genuinely cannot be resolved without
booting, and a snapshot of them is the "true for one boot" half-truth we are
choosing not to ship.

#### L24. Translation depth: JSON lang files, locales, placeholders

**Impact: Medium-High · Complexity: Medium-High**

Statically recoverable translation features the Laravel LSP has and we
still partially lack:

- **JSON lang files.** `lang/{locale}.json` (the "translation string as
  key" style) now completes and resolves go-to-definition, but the
  definition always lands on the top of the file rather than the key's
  actual line, and find-references does not cover JSON keys at all.
- **Locale argument completion.** The `$locale` parameter of `__()`,
  `trans()`, `trans_choice()`, `Lang::get()/choice()/hasForLocale()`
  (positional or named) completes from the locale set derived from
  `lang/*/` directories and `lang/*.json` files.
- **Placeholder parameter completion.** The `:name` placeholders parsed
  from the translation value complete as keys of the replacement array
  (`__('welcome', ['name' => …])`).
- **Multi-locale hover.** Hover already shows a translation key's value
  for the resolved locale; show the value per locale (with a link to
  each file) instead of just the one.
- **Insert missing key quick-fix.** When the unknown-translation-key
  diagnostic fires on a `group.item` key whose `lang/{locale}/group.php`
  array file already exists, offer a quick-fix that inserts the missing
  `'item' => '...'` entry (existing keys as siblings for placement,
  empty string as the value). No fix when the group file itself doesn't
  exist yet; that case still just diagnoses.

#### L27. Legacy `Controller@method` action strings

**Impact: Low · Complexity: Low**

We already complete and resolve the modern `[Controller::class,
'method']` array form (which the Laravel LSP does *not* complete), and
method-name strings inside `Route::controller()` groups. The legacy
`'App\Http\Controllers\FooController@method'` string form gets nothing.
Split on `@`, resolve the class through normal resolution (honoring any
group namespace prefix), and provide completion, go-to-definition, and
an unknown-action diagnostic. Low priority: the string form is
discouraged since Laravel 8.

#### L29. Livewire and Volt component names

**Impact: Low (Livewire projects only) · Complexity: Low**

The component index (class components under `app/Livewire/`, nested
names, and view-based Volt / Livewire v4 single-file components), the
`<livewire:foo-bar>` tag resolution on the Blade side, and hover on a
Livewire component's public properties via `$this` in its view are all
implemented. The remaining gap is the PHP-side triggers: `Volt::route(
'/path', 'component')` (arg 1) and `Route::livewire()` get no
completion, go-to-definition, or unknown-component diagnostic.

#### L30. Eloquent attribute-array key completion

**Impact: Medium · Complexity: Medium**

Our Eloquent string completion covers where-style column arguments and
relation names, but not **attribute-array keys**: `User::create(['name'
=> …])`, `fill()`, `make()`, `update()`, `updateOrCreate()`,
`firstOrCreate()`, `firstOrNew()`, `createOrFirst()` should complete
array keys from the model's column sources (preferring fillable
columns, skipping keys already present in the literal). Also extend the
scalar column list with `firstWhere()`, `whereColumn()`, and the
aggregate methods (`min`/`max`/`sum`/`avg`), and cover the model-level
PHP attributes that take column-name arguments where the installed
framework version has them. The Laravel LSP gets its column list from
`artisan model:show --json` (live DB required); ours comes from the
same static sources the model provider already uses (casts, fillable,
accessors, `@property` tags) — no database needed.

#### L31. String-key rename, highlight, and semantic tokens

**Impact: Low-Medium · Complexity: Medium**

References and go-to-definition already work for the four indexed
string kinds, but the rename, document-highlight, and semantic-token
arms are explicit no-ops. Wiring them up exceeds the Laravel LSP (which
has none of the three): renaming a translation key updates the lang
files across every locale plus all usages; renaming a route name
updates the `->name()` declaration and all usages; highlight and
semantic tokens reuse the existing spans. Renaming a view name implies
moving the Blade file — defer that one until the rest is in place.

#### L32. Config-backed named-resource strings

**Impact: Medium · Complexity: Medium**

Storage disks are one instance of a general pattern: a method argument
names an entry under a known config subtree, and the config scanner
already parses those files. Auth guards (`auth('...')`,
`Auth::guard()`, `->middleware('auth:web')`), cache stores
(`Cache::store()`), log channels (`Log::channel()`), and storage disks
(`Storage::disk()`, test fakes, disk eviction, and `#[Storage]`) already
complete against their config subtree — but all of them
route through the generic `LaravelStringKind::Config` kind rather than
a dedicated one, so they get completion plus the shared config
diagnostics/go-to-definition and nothing family-specific (a "cache
store" hovers with the same generic wording as any other config key).
`Log::stack()` (array values) isn't recognized at all. Generalize into
a declarative table of `(trigger context, config path)` pairs so each
new family is one table row, and cover the rest of the family in one
pass:

- **Database connections** — `DB::connection()`, `->connection()` /
  `$connection` on models and jobs → `database.connections.*`.
- **Queue connections and queues** — `Queue::connection()`,
  `->onConnection()` → `queue.connections.*`; `->onQueue()` names are
  free-form (completion from literals seen elsewhere, no diagnostic).
- **Mailers** — `Mail::mailer()` → `mail.mailers.*`.
- **Broadcast connections** — `Broadcast::connection()` →
  `broadcasting.connections.*`.
- **Rate limiter names** — not config-backed: registered via
  `RateLimiter::for('name', …)` in providers. Scan literal
  registrations (same shape as the macro scanner) and validate
  `throttle:name` middleware parameters and `new RateLimited('name')`
  against the set.

Each family gets the full string-kind treatment for free once wired
as a `LaravelStringKey`: completion, go-to-definition (jump to the
config entry), hover, diagnostics, and references.

#### L39. Unused view and translation key detection

**Impact: Low · Complexity: Medium**

The reverse of the invalid-key diagnostics: keys that are *declared*
but never referenced. The references machinery already finds all
usages of a view name or translation key; inverting it over the
declaration sets yields unused views (no
`view()`/`@include`/`@extends`/mailable reference anywhere) and unused
translation keys. Surface as an opt-in workspace report
(CLI `analyze` flag and/or code lens on the declaration), not as
always-on diagnostics — dynamic construction (`view("emails.$type")`)
makes "unused" inherently a heuristic, so it must read as "no static
reference found," never as an error.

#### L44. Sibling resource registrations and degenerate resource names

**Impact: Low-Medium · Complexity: Medium**

`Route::resource()` and `Route::apiResource()` are recognized, but the
registrations that generate route names the same way are not, so every
`route()` call naming one is reported as an unknown route:

```php
Route::resources(['photos' => PhotoController::class]);
Route::apiResources(['photos' => PhotoController::class]);
Route::singleton('profile', ProfileController::class);
Route::apiSingleton('profile', ProfileController::class);
```

`resources()` / `apiResources()` expand to the same seven (or five)
conventional names per entry. `singleton()` generates `show`, `edit`, and
`update` at `profile`, `profile/edit`, and `profile`, with `store` and
`destroy` added by `->creatable()` / `->destroyable()`.

Separately, a resource name with a leading or trailing slash is trimmed
before the name is derived, where Laravel keeps it and produces a
degenerate-but-real route set: `Route::resource('/photos/', …)` names its
routes `index`/`show` (no `photos.` prefix) rather than `photos.index`.
The same applies to an empty name and to doubled dots (`photos..comments`).
These are absurd inputs that Laravel handles badly too, so the current
tidying is arguably more useful; the divergence is recorded here rather
than fixed because it makes the unknown-route diagnostic disagree with the
framework in both directions.

**Where to look:** `virtual_members/laravel/route_names.rs`, alongside the
existing `resource_registration` handling.

#### L46. `->can()` on a user model the receiver does not name

**Impact: Medium-High · Complexity: Medium-High**

Authorization abilities are recognized on `Gate::allows()`,
`$this->authorize()`, the `can:` middleware parameter, and Blade's `@can`,
but the most common spelling of all, `$user->can('update', $post)`, is
only recognized when the receiver plainly reads as the authenticated user:
a variable or property whose name ends in `user`, or any `user()` call
(`auth()->user()`, `Auth::user()`, `$request->user()`).

An application whose user model is not called `User` writes the same check
and gets nothing. `$customer->can('update', $routine)`,
`$member->canAny([…])`, and `$account->cannot(…)` are all invisible: no
completion inside the string, no hover, no go-to-definition, and no
unknown-ability diagnostic. In a production codebase where every
authorization call is written that way, the whole feature is inert.

The receiver's *type* is what settles this, and `can()` is far too ordinary
a method name to claim on the name alone, so the fix is the mechanism main
already uses for render sites whose receiver only a type settles: emit a
candidate site during extraction and confirm it in a later, type-aware
pass, the way `ViewReceiverSite` /
`Backend::typed_receiver_view_spans` does. A receiver that resolves to the
configured auth model (or to `Illuminate\Contracts\Auth\Access\Authorizable`)
makes the call an authorization check; anything else leaves it alone. The
existing name heuristic stays as the cheap path that needs no type
resolution at all.

**Where to look:** `receiver_is_user_like` in
`symbol_map/extraction/laravel.rs` and its completion-side twin in
`completion/laravel_string_keys.rs`; `ViewReceiverSite` in
`symbol_map/mod.rs` for the confirm-later shape.

#### L49. Unguarded Eloquent mass assignment diagnostic

**Impact: Medium · Complexity: Medium**

`Model::create($request->all())` (and `fill()`/`update()` with an
unfiltered array) silently drops every key the model hasn't declared
`$fillable` for, or throws `MassAssignmentException` in strict mode —
a common first-app footgun with no signal today. Flag a call whose
attribute-array argument is not a literal (so its keys can't be
checked individually) and whose target model has neither `$fillable`
nor `$guarded = []]` declared, on `create()`, `fill()`, `update()`,
`updateOrCreate()`, `firstOrCreate()`, and `firstOrNew()`. The model
resolution and column-source machinery L30 already needs (and the
diagnostic's model-argument matching) share the same receiver-typing
path, so build them together.

**Where to look:** `virtual_members/laravel/model_extraction.rs` for
existing `$fillable`/`$guarded` reads; `diagnostics/` for the
call-site diagnostic pattern used by similarly-shaped checks.

#### L50. "Create route" quick-fix for an unresolved route name

**Impact: Low-Medium · Complexity: Medium**

The unknown-route-name diagnostic (fed by `route_names.rs`) has no
accompanying fix today. Once the diagnostic fires, generate a
`Route::get('{path}', [{Controller}::class, '{method}'])
->name('{name}');` stub appended to `routes/web.php` (or `api.php` if
the call site is `Route::apiResource`-shaped context), inferring path
and controller from the route name's own conventional dashes/dots
where possible and otherwise leaving placeholders.

**Where to look:** `code_actions/` for the code-action registration
pattern used by other "declare the missing thing" fixes;
`virtual_members/laravel/route_names.rs` for the existing scanner
this reuses for the diagnostic and the insertion point.

#### L52. "Create missing view" quick-fix for an unresolved view name

**Impact: Low-Medium · Complexity: Medium**

The `invalid_laravel_view` diagnostic (`diagnostics/mod.rs`, fed by the
same view-key set completion and go-to-definition already resolve
against) has no accompanying fix, the same gap L50 fixes for route
names. The official `laravel/lsp` already ships this exact action:
on an unresolved `view('name')`/`View::make('name')` string it offers
"Create missing view", which creates
`resources/views/{name with dots as slashes}.blade.php` (respecting a
project's configured view paths, not just the default) via a
`workspace/applyEdit` document-create change, then opens the new file.

**Where to look:** `code_actions/` for the code-action registration
pattern used by other "declare the missing thing" fixes; wherever the
view-key set backing `invalid_laravel_view` and view-name completion
is built, for the configured view roots to create the file under.

#### L51. "Convert facade call to dependency injection" refactor

**Impact: Low · Complexity: Medium**

A code action on a facade call inside a class method (`Cache::get(...)`,
`Log::info(...)`) that adds a constructor-promoted property typed to
the facade's underlying contract (resolved the same way `app('cache')`
already resolves to a concrete class for member completion) and
rewrites the call site to `$this->{property}->{method}(...)`. Purely
mechanical once the facade-to-contract resolution already used
elsewhere is in hand; skip call sites already inside a closure that
captures `$this` differently, and skip static/trait contexts where
constructor injection doesn't apply.

**Where to look:** `code_actions/` for the promote-to-constructor
pattern already used elsewhere; `virtual_members/laravel/facade.rs`
for the facade-to-concrete-class resolution to reuse.
