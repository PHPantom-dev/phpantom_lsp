# PHPantom — Tempest

Known gaps and missing features in PHPantom's Tempest support. View
templates already get completion, hover, go-to-definition, and
diagnostics through virtual PHP, the data a view is rendered with is
typed from the calls and View objects that render it, and component
tags, their attributes, and the paths passed to `view()` complete and
navigate. What is still missing is navigating from a component
attribute, documentation for the built-in tags and directives, routes,
and the names Tempest's database and container layers take as strings.

Work out each behaviour from the framework's own source
(`tempest/view`, `tempest/router`, `tempest/database`,
`tempest/container`) and the [view spec](https://tempestphp.com/3.x/internals/view-spec).

`examples/tempest/` is the playground. It installs only the view layer
and what it depends on today, so the router and database items add
`tempest/router` and `tempest/database` to its `composer.json` along
with their demos.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## TM3. Route URIs and route parameters

**Impact: Medium · Complexity: Medium**

```php
#[Get('/books/{book}')]
public function show(Book $book): View { /* ... */ }

uri([BookController::class, 'show'], book: $book->id);
```

Three places name a route or one of its parameters:

- **The first argument of `Tempest\Router\uri()`.** Complete the routes
  the project declares (the `#[Get]`, `#[Post]`, … attributes on
  controller methods, showing the HTTP method and the action), and
  navigate from the argument to the action method. `uri()` takes an
  array callable, an invokable controller's class name, or a URI string
  starting with `/`, so a string completes as either and navigates by
  whichever it matches.
- **The named arguments after it.** They fill the route's `{name}`
  placeholders. Complete them from the placeholders of the route the
  first argument picks, navigate from one to the action's parameter, and
  include them when that parameter is renamed.
- **The placeholders in the route attribute itself.** Navigate from
  `{book}` to the `$book` parameter of the method the attribute sits on,
  and complete the method's parameter names inside the braces.

Collect the routes in the same pass that indexes classes rather than
walking every attribute on demand, the way Yii's routes are collected
in `src/yii/routes.rs`. The built-in attributes all implement
`Tempest\Router\Route`. If the router's discovery accepts any class
implementing it, recognise routes by the interface rather than by a
fixed list of attribute names, so a project's own route attributes are
found too.

---

## TM4. Model property names in query builders and model helpers

**Impact: Medium · Complexity: Medium**

```php
query(Book::class)->select()->whereIn('isbn', $isbns)->where('title', $title);
Book::updateOrCreate(['isbn' => $isbn], ['title' => $title]);
Book::create(title: $title, isbn: $isbn);
```

Tempest's query builders take a model's property names as strings
(`whereField()`, the `whereIn()`/`whereNull()`/`whereLike()` family and
its date helpers, and `where()`/`andWhere()`/`orWhere()` when the string
is a bare field), and the `IsDatabaseModel` helpers take them as array
keys (`findOrNew()`, `updateOrCreate()`) or named arguments (`create()`,
`new()`, `find()`). Complete those names from the model's properties
and navigate from a name to the property. The model comes from the
builder's generic type (`query()` returns `QueryBuilder<TModel>`, and
`select()` a `SelectQueryBuilder<TModel>`), which the type engine
already resolves, so read it from there rather than from the text of
the `query()` call. `where()` treats a string containing a space or an
operator as raw SQL and passes it to `whereRaw()`, so leave those, and
the `*Raw()` methods, alone. Check `orderBy()`, `groupBy()`, and
`having()` against `tempest/database` too, and include whichever take a
field name.

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
