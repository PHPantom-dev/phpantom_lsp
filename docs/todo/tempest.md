# PHPantom — Tempest

Known gaps and missing features in PHPantom's Tempest support. View
templates already get completion, hover, go-to-definition, and
diagnostics through virtual PHP, the data a view is rendered with is
typed from the calls and View objects that render it, and component
tags, their attributes, and the paths passed to `view()` complete and
navigate. What is still missing is route URIs that a project's own
decorators change.

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
