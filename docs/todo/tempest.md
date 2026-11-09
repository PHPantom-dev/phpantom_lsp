# PHPantom — Tempest

Known gaps and missing features in PHPantom's Tempest support. View
templates already get completion, hover, go-to-definition, and
diagnostics through virtual PHP, the data a view is rendered with is
typed from the calls and View objects that render it, and component
tags, their attributes, and the paths passed to `view()` complete and
navigate. The remaining gap is completing and navigating the data
passed to a view at the call site.

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

## TM8. The data `view()` passes completes and navigates

**Impact: Low-Medium · Complexity: Low-Medium**

```php
return view('./books.view.php', books: $books, title: 'Books');
```

The named arguments `view()` passes become the template's variables,
and the template already types `$books` from them, but the call site
gets nothing back: completion inside the call offers only `params:`,
and go-to-definition and hover on `books:` find nothing. Laravel's
`view('books', ['books' => …])` and Yii's `render('index', ['form' => …])`
already do this (a **View parameter** hover and a jump to the
variable's first read in the template). Complete the variables the
template reads that the call does not pass yet, and navigate and hover
from a name to the variable, reusing the reverse of the lookup that
types the template (`src/tempest/view_data.rs`). A custom View object's
constructor arguments are ordinary parameters and keep their handling.
