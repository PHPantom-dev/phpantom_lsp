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

`examples/tempest/` is the playground. It installs the view and router
layers. The database items add `tempest/database` to its `composer.json`
along with their demos.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## TM6. `query(Book::class)` builds a query for `Book`

**Impact: Medium · Complexity: Low**

```php
$books = query(Book::class)->select()->where('title', $title)->all();
// array<class-string<Book>>, should be array<Book>
```

`Tempest\Database\query()` declares `@template TModel` and
`@param TModel $model`, so a class name binds `TModel` to
`class-string<Book>`, and every builder after it, and every record its
`all()` and `first()` return, is a class name instead of a `Book`. The
builder's own constructor says what is meant
(`@param class-string<TModel>|TModel|string $model`), and
`Book::select()` already gives `SelectQueryBuilder<Book>`. Read
`query()`'s parameter the way the constructor declares it, so a class name
binds `TModel` to the class, and offer the docblock fix upstream.

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

---

## TM7. A project's own route decorators change the route's URI

**Impact: Low · Complexity: Medium**

```php
#[Attribute(Attribute::TARGET_CLASS | Attribute::TARGET_METHOD)]
final class Admin implements RouteDecorator
{
    public function decorate(Route $route): Route
    {
        return new Prefix('/admin')->decorate($route);
    }
}

#[Admin]
final class BookController
{
    #[Get('/books/{book}')] // the route is /admin/books/{book}
    public function show(Book $book): View { /* ... */ }
}
```

Route discovery applies every attribute that implements
`Tempest\Router\RouteDecorator`, method ones before class ones, but only
`Tempest\Router\Prefix` is read for the URI it adds. A project's own
decorator that rewrites the URI is skipped, so its routes are recorded
without the prefix, and a `uri()` literal that includes it finds no
route. Read a decorator whose `decorate()` hands the route to a `Prefix`
it builds from a literal, or assigns `$route->uri` from a literal joined
with the old URI, as that prefix.
