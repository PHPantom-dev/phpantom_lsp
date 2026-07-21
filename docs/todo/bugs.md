# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

## Background index publish can clobber files opened and edited mid-index

The full background index snapshots the set of already-parsed URIs
when it starts, parses everything else (Phase 2 reads straight from
disk), and publishes all results in one batch merge at the end. A file
that is opened *after* the snapshot and edited *during* the parse
window gets its fresh `did_change` state (symbol map, classes,
resolved names) overwritten by the stale disk parse when the batch
publishes. Hover, diagnostics, and references for that file are then
computed from pre-edit content until the next keystroke or save
re-parses it. The window is the full index duration (tens of seconds
on large projects), and the same race exists for any
`ensure_workspace_indexed` run triggered by find-references. Fix: at
batch-publish time, skip updates whose URI is currently in
`open_files` (the open buffer's `update_ast` already produced newer
state), or re-check open-file content per update before applying.

## Go-to-implementation results for vendor classes are session-dependent once the workspace index is ready

`find_implementors` returns early after the reverse-inheritance-index
phase once the workspace index is ready, intentionally skipping the
vendor/classmap/stub scan phases (the tests assert vendor
implementors are not returned). But the reverse inheritance index is
populated by *every* `update_ast`, including vendor files parsed
lazily during class resolution. So a vendor implementor that happens
to have been loaded earlier in the session (because the user hovered
or completed something that resolved it) **does** appear in
go-to-implementation and type hierarchy results, while an identical
vendor implementor that was never loaded does not. Results depend on
session history. Pick one behaviour and enforce it: either filter
vendor FQNs out of the index-ready fast path (deterministic user-only
results, matching the tests' intent), or keep a bounded vendor
fallback so vendor implementors are always included. If user-only is
chosen, consider whether stub implementors (e.g. SPL classes
implementing `Iterator`) need the same treatment.

## By-reference closure capture propagation ignores named arguments

When a closure is passed as a *named* argument to a call
(`c(callback: function () use (&$foo) { ... })`), the by-reference
capture propagation matches the argument to a parameter by its
position in the argument list rather than by the argument's name.
If the named argument is not in its natural position, the wrong
parameter's `@param-immediately-invoked-callable` /
`@param-later-invoked-callable` tag is consulted, so the outer
variable's type is either propagated when it should not be or left
stale when it should update. Match named arguments to parameters by
name before deciding whether the callable is immediately invoked.

## Eloquent models without a migration or schema dump lack the implicit `id` primary key

Every Eloquent model has an implicit `id` primary key (Laravel's
default `$primaryKey`), but PHPantom only synthesizes model
properties from migrations and schema dumps. A model whose table has
neither (no `create_<table>` migration, no `database/schema` dump)
therefore exposes none of its columns, and even the always-present
primary key is missing, so `$model->id` is flagged
`Property 'id' not found`. Reproduces in `examples/laravel` on
`App\Models\Bakery` (there is no bakeries migration): `$bakery->id`
reports a false positive at
`app/Http/Controllers/BakeryController.php`. Synthesize the primary
key (respecting a model's `$primaryKey` / `$incrementing` overrides)
for every Eloquent model regardless of whether schema data is
available, so the default `id` is always known.

## Blade attribute directives corrupt everything after them in the virtual PHP

The Blade preprocessor recognizes `class`, `style`, `checked`,
`selected`, `disabled`, `readonly`, and `required` as directives (they
appear in `match_directive`), but none of them has a case in
`preprocess()`'s big if/else chain in `src/blade/preprocessor.rs`, so
they fall into the generic default branch:

```rust
} else {
    replacement = format!(" {}; ", translate_directive(directive));
    next_mode = Mode::Php;
}
```

Unlike the other directives handled here, this branch does not consume
the directive's parenthesized argument list (no `DirectiveArgs` mode)
and does not return to `Mode::Html`. When one of these directives is
used the way Laravel actually supports them, as a conditional
attribute inline inside an HTML tag, e.g.
`<div @class(['collapse', 'in' => $errors->has('x')]) id="foo">`, the
preprocessor emits the boilerplate statement and then keeps parsing
the rest of the line, and every subsequent line, as raw PHP: the
argument list, the closing `>`, and all following HTML/Blade markup.
This produces dozens of cascading `syntax_error` diagnostics per file
(one real-world file produced 252) until something coincidentally
closes the runaway PHP mode. Confirmed against two production Laravel
codebases (dozens of affected files combined) once the newly-added
`config/view.php` path support (see "Read view folder from config")
started scanning their non-default view directories for the first
time. Fix: give these seven directives their own case that treats them
as an expression directive, consumes the argument list like
`DirectiveArgs`, and returns to `Mode::Html` afterward (they never take
a body/`@end...` counterpart).

## `@use` and `@inject` corrupt everything after them in the virtual PHP

`use` and `inject` are recognized directive names (`match_directive`
in `src/blade/directives.rs`) and `translate_directive` has entries
for both (`"use" => "use "`, `"inject" => "$"`), but neither has a
case in `preprocess()`'s if/else chain in `src/blade/preprocessor.rs`,
so both fall into the same generic default branch described above:
the parenthesized argument list is left unconsumed and the parser
stays in `Mode::Php` for the rest of the template, corrupting
everything after it. Unlike the seven attribute directives fixed
above, neither can be fixed by simply routing them through the
existing `DirectiveArgs` consume-and-return-to-`Html` path, because
their real-world semantics require actually parsing the argument
string, not just discarding it:

- `@use('App\Models\Post')` / `@use('App\Models\Post as Article')`
  must become a real `use App\Models\Post;` /
  `use App\Models\Post as Article;` statement so the imported name
  resolves; emitting `use ('App\Models\Post');` (treating the string
  as a plain expression) is not valid PHP import syntax.
- `@inject('var', 'Class')` must become `$var = app('Class');` (or
  similar) so the injected variable gets the class's type; the
  `translate_directive` mapping of `"inject" => "$"` alone does not
  produce a complete assignment.

Fix: give `@use` and `@inject` their own preprocessor cases that
parse the string-literal argument(s) out of the parens and emit the
correct real PHP construct, then return to `Mode::Html`.

## Blade component bound attributes (`:prop="$expr"`) are invisible to variable-usage tracking

Laravel's Blade component tag compiler treats any HTML-like tag
attribute written as `:name="$expr"` (or the `:$var` shorthand) as a
bound prop whose value is a real PHP expression, evaluated and
passed to the component/child scope. This applies to first-party
`<x-...>` components and to package-registered tag namespaces (for
example Livewire's `<livewire:...>`) alike. The Blade preprocessor
(`src/blade/preprocessor.rs`) has no handling for this syntax at
all: everything inside an HTML tag is masked as opaque literal text,
so the `$expr` in `:src="$image"` or `:key="$item->id"` is never
emitted as PHP and never seen by the forward walker. Confirmed
against a production Laravel codebase (`<x-backoffice::img.size
:src="$image" ... />`, `<livewire:app-channels.edit-channel
:key="$item->id" ... />`, and similar patterns across 13 files):
every variable whose only use is inside a bound attribute is
reported as a false-positive `unused_variable`, and — since the
expression is never evaluated — go-to-definition, hover, and
completion inside `:prop="..."` don't work either.

Fix is more involved than the directive-argument gaps above: it
needs new handling in `Mode::Html` that recognizes a `:name="..."`
(or `:$var`) attribute while inside a tag's `<...>` span, extracts
the quoted expression, and emits it as PHP (e.g. via the same
`blade_directive(...)` pass-through used for attribute directives)
without disturbing the surrounding tag markup, which must stay
masked. Care is needed to scope the match to attribute position
only (inside `<tag ...>`, after whitespace, not preceded by another
identifier character) so it doesn't misfire on unrelated colons in
attribute values (e.g. `href="mailto:x"`, a `10:30` time string, or
CSS/JS content inside the tag).
