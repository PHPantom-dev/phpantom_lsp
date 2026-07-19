# PHPantom — Blade

This document is the implementation plan for Laravel Blade template
support in PHPantom. For Eloquent model support see `laravel.md`.
For general architecture see `ARCHITECTURE.md`.

---

## Philosophy

- **No application booting.** Consistent with `laravel.md`. We
  never run PHP or boot a Laravel application.
- **Signatures over call-site scanning.** A template's variable types
  come from its declared contract: the Bladestan-compatible signature
  chain — `@bladestan-signature` docblock, first docblock before
  template code, `@props`, component class constructors (item 21). The
  template declares what it expects; call sites are then *validated*
  against that contract (item 24), exactly as a function signature
  works. Inferring types *from* call sites inverts the contract and
  produces "true for one caller" types, so it is not the foundation —
  but a best-effort call-site inference fallback for unannotated
  projects is planned as a late addition (item 25), layered strictly
  below every declared source. Projects running Bladestan (a PHPStan
  extension for Blade template analysis) get the full contract model
  in both the editor and CI from the same annotations.
- **Discovery is just directory walks.** Scanning `resources/views/`
  and `app/View/Components/` (plus `app/Livewire/`) at init time is
  the full extent of external Blade file discovery. Paths are converted
  to view names and component names via string transforms.
- **PSR-4 is for class source lookup, not discovery.** Once we know an
  FQN (e.g. `App\View\Components\Alert`), we use the existing
  `find_or_load_class` pipeline to read its source. We do not use
  PSR-4 to discover component names.
- **Graceful degradation.** Unknown directives become comments. Failed
  component resolution produces comments. The user always gets partial
  completions rather than a broken file. The preprocessor must never
  produce invalid PHP.

---

## Overview

Blade templates (`.blade.php`) mix HTML, Blade directives, component
tags (`<x-alert>`, `<livewire:counter>`), and embedded PHP. The
mago-syntax parser only understands pure PHP. The strategy:

1. Preprocess `.blade.php` files into valid PHP.
2. Feed the virtual PHP through the existing pipeline (parser,
   resolver, completion, definition).
3. Map LSP response positions back to the original Blade file via a
   source map.

---

## Phase 1: Blade-to-PHP Preprocessor

The core preprocessor is implemented in `src/blade/`. It transforms
Blade templates into virtual PHP line-by-line, with a source map for
coordinate translation. The LSP pipeline (`with_file_content`,
`update_ast`, `did_close`) transparently handles Blade files.

---

## Phase 2: Component Support

### 8. Blade-aware code actions

Code actions are currently disabled for `.blade.php` files because
text edits target virtual PHP coordinates and actions like "Import
class" insert `use` statements at the top of the file rather than
inside a `@php` / `<?php` block. Re-enable code actions with:

- Range translation (virtual PHP → Blade) for all text edits.
- Blade-aware code generation (e.g. insert `use` inside `@php`).
- Filtering out actions that don't make sense in Blade context.

### 9. Template and component file discovery

At `initialized` time (alongside PSR-4 and classmap loading), scan
the filesystem to build three maps.

New file: `src/blade/discovery.rs`

#### 9a. View name map

Recursively scan `resources/views/` for `*.blade.php` files. Build
a map of dot-notation view names to file paths:

- `resources/views/users/index.blade.php` → `"users.index"`
- `resources/views/components/alert.blade.php` → `"components.alert"`

Store as:

```rust
/// View dot-name -> file path.
pub(crate) blade_views: Arc<Mutex<HashMap<String, PathBuf>>>,
```

#### 9b. Class-based component map

Recursively scan `app/View/Components/` for `*.php` files. Convert
file paths to kebab-case component names and FQNs:

- `app/View/Components/Alert.php` → name `"alert"`,
  FQN `"App\\View\\Components\\Alert"`
- `app/View/Components/Forms/Input.php` → name `"forms.input"`,
  FQN `"App\\View\\Components\\Forms\\Input"`

Index components (where directory name matches file name) should be
registered both ways:

- `app/View/Components/Card/Card.php` → name `"card"` (index) and
  `"card.card"` (explicit)

Store as:

```rust
/// Component kebab-name -> FQN.
pub(crate) blade_components: Arc<Mutex<HashMap<String, String>>>,
```

#### 9c. Livewire component map

Recursively scan `app/Livewire/` for `*.php` files. Convert file
paths to dot-notation component names and FQNs:

- `app/Livewire/Counter.php` → name `"counter"`,
  FQN `"App\\Livewire\\Counter"`
- `app/Livewire/Admin/Users.php` → name `"admin.users"`,
  FQN `"App\\Livewire\\Admin\\Users"`

Store as:

```rust
/// Livewire component name -> FQN.
pub(crate) livewire_components: Arc<Mutex<HashMap<String, String>>>,
```

#### 9d. Workspace root dependency

All three scans depend on `workspace_root`. Run them in `initialized`
after the existing Composer parsing, gated on
`workspace_root.is_some()`.

### 10. `<x-component>` tag parsing in preprocessor

New file: `src/blade/components.rs`

The preprocessor detects `<x-name ...>` and `</x-name>` tags and
converts them to PHP.

#### 10a. Opening tags

Parse `<x-component-name attr="val" :attr="$expr" ...>` or
`<x-component-name ... />` (self-closing).

1. Extract the component name (everything between `<x-` and the first
   whitespace or `>`/`/>`).
2. Look up the name in `blade_components`. If found, resolve the FQN.
3. Extract attributes:
   - `attr="literal"` → named arg with string value
   - `:attr="$expr"` → named arg with PHP expression value
   - `::attr="expr"` → ignored (Alpine.js passthrough)
   - Bare `attr` → named arg with `true`
   - `:$var` (short syntax) → named arg `var: $var`
4. Convert attribute names from kebab-case to camelCase for the
   constructor call.
5. Emit `$component = new \FQN(camelAttr: value, ...);`

If the component is not found in `blade_components`, check if it's an
anonymous component (exists in `blade_views` under `components.`
prefix). For anonymous components, emit a comment but still expose
`$attributes` and `$slot`.

For `<x-dynamic-component :component="$name" ...>`, emit
`echo $name;` so the expression gets parsed, but do not try to
resolve a target component.

#### 10b. Closing tags

`</x-name>` becomes a comment: `/* /x-name */`

#### 10c. Named slots

`<x-slot:title>` → `$title = new \Illuminate\Support\HtmlString('');`
`</x-slot>` → comment

#### 10d. Implicit component variables

When inside a component tag region (between opening and closing tags),
inject:

```php
/** @var \Illuminate\View\ComponentAttributeBag $attributes */
$attributes = new \Illuminate\View\ComponentAttributeBag([]);
/** @var \Illuminate\Support\HtmlString $slot */
$slot = new \Illuminate\Support\HtmlString('');
```

### 11. `<livewire:component>` tag parsing

Parse `<livewire:name :attr="$expr" ...>` or
`<livewire:name ... />`.

1. Extract the component name (everything between `<livewire:` and
   the first whitespace or `>`/`/>`).
2. Look up in `livewire_components`. If found, resolve the FQN.
3. Extract attributes (same rules as `<x-...>`).
4. Emit `$component = new \FQN();` followed by property assignments
   for each attribute: `$component->attrName = $expr;`.

Livewire attribute names use camelCase on the class, so apply the
same kebab-to-camelCase conversion.

### 12. `@props` and `@aware`

#### 12a. `@props`

`@props(['type' => 'info', 'message'])` becomes:

```php
$type = 'info';
$message = null;
/** @var \Illuminate\View\ComponentAttributeBag $attributes */
$attributes = new \Illuminate\View\ComponentAttributeBag([]);
/** @var \Illuminate\Support\HtmlString $slot */
$slot = new \Illuminate\Support\HtmlString('');
```

The preprocessor parses the array literal in the `@props()`
argument to extract variable names and default values. Variables
listed without a key-value pair (just `'message'`) get a `null`
default.

#### 12b. `@aware`

`@aware(['color' => 'gray'])` → `$color = 'gray';`

Same parsing as `@props` but without the `$attributes`/`$slot`
injection.

### 13. Component and view name completion

#### 13a. `<x-` completion

When the user types `<x-` in a Blade file, offer completions from:

- `blade_components` map (class-based components, kebab-case names)
- Anonymous component templates: entries in `blade_views` whose key
  starts with `"components."`, with the prefix stripped and dots
  preserved (e.g. `"components.forms.input"` → `"forms.input"`)

Detection: check if the characters before the cursor match
`<x-` (possibly with a partial name typed). This is a Blade-level
context check done before the normal PHP completion pipeline.

Items should use `CompletionItemKind::Module` or `::Class` depending
on whether they're anonymous or class-backed.

#### 13b. `<livewire:` completion

Same pattern. When the user types `<livewire:`, offer completions
from the `livewire_components` map.

#### 13c. `@include('` and `@extends('` view name completion

When the cursor is inside the string argument to `@include`,
`@includeIf`, `@includeWhen`, `@includeUnless`, `@includeFirst`,
`@extends`, `@each`, or a `view()` function call, offer completions
from the `blade_views` map (dot-notation view names).

Detection: look for `@include('`, `@extends('`, or `view('` before
the cursor and check that the cursor is inside the quotes. The
trigger characters `'` and `"` are already registered.

#### 13d. Component attribute completion

When the cursor is inside a `<x-component ` tag (after the component
name, before `>` or `/>`), resolve the component class and offer its
constructor parameter names as kebab-case attribute completions.

Offer both plain and `:` prefixed variants:
- `message` (string literal)
- `:message` (PHP expression)

For Livewire components, offer the class's public property names as
attribute completions.

### 14. Tests

Create `tests/blade_components.rs`:

- `<x-alert>` resolves to `App\View\Components\Alert`
- `<x-forms.input>` resolves to `App\View\Components\Forms\Input`
- `<x-card>` resolves to index component
  `App\View\Components\Card\Card`
- `<livewire:counter>` resolves to `App\Livewire\Counter`
- Anonymous component detection
- `<x-dynamic-component>` does not crash
- Attribute parsing: string, expression, Alpine passthrough, bare,
  short syntax

Extend `tests/completion_blade.rs`:

- `<x-` triggers component name completions
- `<livewire:` triggers Livewire component name completions
- `@include('` triggers view name completions
- `<x-alert ` triggers attribute completions
- `$component->` after component instantiation
- `$attributes->` in component templates

---

## Phase 3: Cross-File View Intelligence

### 15. Go-to-definition for view names and components

#### 15a. View name go-to-definition

Inside `@include('users.index')`, `@extends('layouts.app')`, or
`view('welcome')`:

1. Extract the view name string at the cursor position.
2. Look up in `blade_views`.
3. Return a `Location` pointing to the resolved file.

#### 15b. Component tag go-to-definition

On `<x-alert>`:

1. Extract the component name.
2. Look up in `blade_components` to get the FQN.
3. Use `find_or_load_class` + `fqn_uri_index` to find the
   source file.
4. Return a `Location` pointing to the class definition.

On `<livewire:counter>`:

1. Same pattern using `livewire_components`.

### 16. Signature merging for `@extends`

When template A contains `@extends('layouts.app')`:

1. Resolve `layouts.app` via `blade_views` to a file path.
2. Read or preprocess that file.
3. Extract `@var` declarations from its `@php` blocks.
4. Merge those declarations into template A's virtual PHP prologue,
   following the Bladestan covariance model:
   - Variables only in child: use child type.
   - Variables only in parent: use parent type.
   - Variables in both: child may narrow but not widen.
   - Walk the chain recursively if the parent also `@extends`.

This gives child templates access to the parent's declared
variables without the user redeclaring them.

### 17. Component class to template variable typing

For class-based components, when editing the component's Blade
template:

1. Determine which component class backs this template. Convention:
   `resources/views/components/alert.blade.php` is backed by
   `App\View\Components\Alert`.
2. Load the class via `find_or_load_class`.
3. Read public properties and constructor parameter types.
4. Inject those as `@var` declarations in the virtual PHP prologue
   (unless the template already has explicit `@var` or `@props`).

### 18. Tests

Create `tests/definition_blade.rs`:

- Go-to-definition on `@include('users.index')` → view file
- Go-to-definition on `@extends('layouts.app')` → layout file
- Go-to-definition on `<x-alert>` → component class
- Go-to-definition on `<livewire:counter>` → Livewire class

Extend `tests/completion_blade.rs`:

- Variables from parent layout available in child via `@extends`
- Component class constructor types available in template

---

## Phase 4: Blade Directive Completion

### 19. Directive name completion

When the user types `@` in a Blade file (outside `{{ }}`, `@php`
blocks, and string literals), offer completions for all known Blade
directives with snippet templates.

Each completion inserts a snippet with tab stops:

```
@if ($1)
    $0
@endif
```

```
@foreach ($1 as $2)
    $0
@endforeach
```

```
@include('$1')
```

```
@props([$1])
```

```
@inject('$1', '$2')
```

```
@php
$0
@endphp
```

Detection: The `@` trigger character is already registered. In
`handle_completion`, check `is_blade_file` and that the cursor is in
an HTML/directive context (not inside `{{ }}`, not inside a `@php`
block, not inside a string literal).

### 20. Tests

Extend `tests/completion_blade.rs`:

- `@` triggers directive name completions
- `@if` partial triggers filtered directive completions
- No directive completion inside `{{ }}` or `@php` blocks

---

## Phase 5: Template Contracts and Cross-File Flow

This phase aligns PHPantom's Blade understanding with Bladestan (a
PHPStan extension for statically analyzing Blade templates). The two
tools share one contract model: the same annotation gives autocomplete
and hover in the editor (PHPantom) and type checking in CI (Bladestan).
Where Bladestan defines a concept (signature chain, covariant merging,
call-site validation), we implement the same semantics so the
ecosystem converges on one way to type a template.

### 21. Template signature resolution chain

Implement a signature resolution priority, matching Bladestan's model,
as the source of a template's variable types:

1. **Backed component class** — public properties and constructor
   parameters (item 17), merged with the blade-side signature.
2. **`@bladestan-signature` docblock** — the canonical contract. The
   marker tag is recognized and the `@var` tags in that docblock
   become the template's declared variables.
3. **First docblock before template code** — treated as an implicit
   signature when no marker is present (zero-change migration for
   codebases that already carry `@var` blocks).
4. **`@props`** — default-value type inference for anonymous
   components (item 12).
5. **`mixed`** — otherwise.

The `@var` parsing itself already works through the preprocessor;
the work is the chain, the one-signature-per-file rule, and treating
the signature as a *declaration set* rather than scattered
assertions. Nullable-not-optional semantics follow Bladestan: a
variable that may be absent is declared `?Type`, not omitted.

### 22. Cross-file `@section` / `@stack` name intelligence

`@section`/`@hasSection`/`@sectionMissing`/`@yield` and
`@push`/`@prepend`/`@stack` name arguments are cross-file string
keys: yields and stacks are declared in layouts, filled in children.
Index section and stack names per template (alongside the discovery
maps of item 9, recording the `@extends` target), then provide:

- completion of section/stack names inside child templates from the
  resolved layout chain, and vice versa in layouts from known
  children;
- go-to-definition between `@section('x')` and its `@yield('x')`;
- an unknown-section diagnostic when a child fills a section its
  layout chain never yields (dynamic names skip, as always).

### 23. Custom directive discovery

`Blade::directive('datetime', …)`, `Blade::if('env', …)`, and
component namespace registrations (`Blade::componentNamespace()`,
`Blade::anonymousComponentPath()`) in app and package service
providers declare project-specific directives. Scan literal
registrations — the same provider-scanning shape as the macro
scanner — so that:

- known custom directives stop degrading to comments in the
  preprocessor and instead map to expression-preserving PHP (their
  argument is still type-checked);
- `Blade::if('admin')` synthesizes the full family (`@admin`,
  `@elseadmin`, `@endadmin`, `@unlessadmin`);
- directive name completion (item 19) includes them;
- registered component namespaces/paths extend the discovery maps of
  item 9.

### 24. `view()` call-site validation

The diagnostics counterpart of item 21, matching Bladestan's
call-site validation rule: where a template has a
declared signature, a `view('name', [...])` call (and
`View::make()`, mailable content, `Route::view()` data,
`@include('name', [...])` inside templates) with a literal array
argument is checked against the merged signature as an array shape —
missing required variables, unknown extras, and type mismatches each
get a diagnostic. Templates without a signature produce no call-site
diagnostics. This gives the editor the same errors Bladestan reports
in CI, live while typing, from one annotation.

### 25. Call-site variable inference (late addition)

For projects using neither signatures nor Bladestan: infer a
template's variable types from the `view()` call sites that
reference it (literal array keys, `compact()`, `->with()` chains),
as the **lowest-priority** source in the item 21 chain — any
declared source shadows it entirely, and multiple call sites union
per variable. This is deliberately last: it is the inverted-contract
model and inherits its weaknesses (types are "true for the callers
we found", dynamic view names contribute nothing). It exists so a
completely unannotated project still gets useful completion inside
`{{ $user-> }}`, and as a nudge path — hover can suggest promoting
the inferred types into a signature docblock.

---

## Implementation Sequence

Phase 1 is complete (steps 1-3): the preprocessor, LSP pipeline
integration, source mapping, `$loop`/`@session`/`@error`/`@context`
implicit variables, stub directives, verbatim regions, `languageId`
check, and code action suppression are all shipped.

The remaining steps build on the existing preprocessor:

### Step 4: Discovery (items 8-9)

Implement `src/blade/discovery.rs`. Scan `resources/views/`,
`app/View/Components/`, `app/Livewire/` at init time. Add the three
new maps to `Backend`.

**Deliverable:** Maps are populated and logged at startup.

### Step 5: Component tag parsing (items 10-12)

Implement `src/blade/components.rs`. Parse `<x-...>` and
`<livewire:...>` tags. Handle `@props`, `@aware`, named slots.

**Deliverable:** `$component->` after `<x-alert>` produces
completions from the Alert class. `$attributes->` works in component
templates.

### Step 6: Name completions (item 13)

Implement `<x-`, `<livewire:`, `@include('`, and component attribute
completions.

**Deliverable:** Typing `<x-` shows available components. Typing
`@include('` shows available views. Typing attributes inside
`<x-alert ` shows constructor parameter names.

### Step 7: Directive completion (item 19)

Implement `@` directive name completion with snippets.

**Deliverable:** Typing `@` in a Blade file shows all known
directives with snippet templates.

### Step 8: Cross-file intelligence (items 15-17)

Implement go-to-definition for view names and component tags.
Implement `@extends` signature merging. Implement component class to
template variable typing.

**Deliverable:** Ctrl-click on `@include('users.index')` jumps to
the file. Parent layout variables are available in child templates.

### Step 9: Template contracts (items 21-25)

Implement the signature resolution chain, then call-site validation
on top of it. Section/stack intelligence and custom directive
discovery are independent and can land in either order. Call-site
inference (item 25) comes last, after every declared source works.

**Deliverable:** A template with a `@bladestan-signature` docblock
gets typed completion for its declared variables, and a `view()`
call missing a required variable gets a diagnostic.

---

## Editor Integration Notes

### File extension detection

The server activates Blade preprocessing when:
- The URI ends with `.blade.php`, OR
- The `languageId` in `did_open` is `"blade"`.

### Zed extension

The Zed extension (`zed-extension/extension.toml`) currently
registers `languages = ["PHP"]` and is planned to merge into the
official Zed PHP extension, so it will not grow Blade support. The
Blade language registration (tree-sitter grammar, `.blade.php`
association, `languageId: "blade"` wiring) belongs to the separate
`zed-laravel` extension — see `zed-laravel/todo.md` items Z2/Z3.

### Other editors

- **VS Code:** Extensions like Laravel Blade Snippets set
  `languageId` to `"blade"`. PHPantom's VS Code integration would
  need to register for both `"php"` and `"blade"` language IDs.
- **Neovim:** `lspconfig` can be configured to send `.blade.php`
  files to PHPantom with the correct `languageId`.