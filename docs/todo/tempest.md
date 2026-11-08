# PHPantom — Tempest

Known gaps and missing features in PHPantom's Tempest support. View
templates already get completion, hover, go-to-definition, and
diagnostics through virtual PHP, the data a view is rendered with is
typed from the calls and View objects that render it, and component
tags, their attributes, and the paths passed to `view()` complete and
navigate. What is still missing is navigating from a component
attribute, documentation for the built-in tags and directives, and the
names Tempest's container forwards to callables.

Work out each behaviour from the framework's own source
(`tempest/view`, `tempest/router`, `tempest/database`,
`tempest/container`) and the [view spec](https://tempestphp.com/3.x/internals/view-spec).

`examples/tempest/` is the playground. It installs the view, router, and
database layers.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## TM1. Component attribute names navigate to the component's variable

**Impact: Low-Medium · Complexity: Low**

```html
<x-book-card :book="$book" show-author />
```

Attribute names on a component tag already complete from the variables
the component's view reads (`src/tempest/components.rs`), but
go-to-definition on `show-author` or `:book` finds nothing. Navigate
from an attribute name to the first read of `$showAuthor` (or `$book`)
in the component's view, matching the kebab-case conversion completion
uses, and hover the attribute with the type the component sees.
Built-in directives (`:if`, `:foreach`, …) and `:is` on
`<x-component>` keep their current handling.

---

## TM2. Documentation for built-in components and directives

**Impact: Low · Complexity: Low**

```html
<x-form action="/books"><x-input name="title" /></x-form>
<li :foreach="$books as $book" :if="$book->published">…</li>
```

Completion offers the directives (`:if`, `:elseif`, `:else`, `:isset`,
`:foreach`, `:forelse`, `:apply`, `:as`) and the built-in tags, but
neither the completion items nor hover say what they do. Add a short
description and a link to the matching section of the view spec for
each, shown in the completion item's documentation and on hover over
the directive or tag name.

---

## TM5. Named arguments `Container::invoke()` forwards

**Impact: Low · Complexity: Low-Medium**

```php
$container->invoke([BookImporter::class, 'import'], path: $path);
```

`Tempest\Container\Container::invoke()` calls the callable it is given
and forwards its remaining named arguments to it. Complete those names
from the callable's parameters and navigate from one to its parameter.
Laravel's `app()->call([Foo::class, 'bar'], ['param' => …])` forwards
the same way through an array, so build the callable lookup once and
let both use it.
