# PHPantom — Yii

Known gaps and missing features in PHPantom's Yii support. Yii 3 is
built from small, typed components with little magic, so the generic
engine already types most of it. On top of that, route names, route
arguments, view names, and the members and arguments DI definitions name
complete, navigate, and are checked, and path aliases navigate to their
declarations. What is still missing is the navigation between the rest
of the names Yii keeps in configuration (`$params` keys) and the code
and files they point to.

The items below are what the Yii team asked for in
[discussion #505](https://github.com/PHPantom-dev/phpantom_lsp/discussions/505).
`examples/yii/` (generated from `yiisoft/app`) is the playground, with a
route, a view, a DI definition, and an alias of its own for each item
below to light up, and `yiisoft/demo-blog` is the real-world reference.
Analysing the demo blog turned up no Yii-specific diagnostics: Yii 3's
DI, router, and view layer are plain typed PHP, so what is missing is
navigation, which `analyze` cannot measure.

**Benchmarks.** `benches/memory_usage.py` measures memory on
`yiisoft/demo-blog` (`memory_yii_action`), next to the Laravel and
Symfony scenarios. `benches/yii_completion.rs` measures name completion
on a generated mid-sized application, with a case per kind of name, and
every item that adds a completion adds its case to it.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## Y6. `$params` keys in configuration

**Impact: Medium · Complexity: Medium**

```php
// config/common/di/application.php
'name' => $params['application']['name'],
```

`yiisoft/config` hands every config file a `$params` array merged from
the `params` group (`config/common/params.php`, the web or console
file, and the environment's override). Every app generated from
`yiisoft/app` reads it in its DI files. Complete the keys at each level,
navigate from a key to the entry that declares it (the last file in
merge order wins), type the read as the declared value rather than
`mixed`, and report a key no params file declares. The merge order
comes from `yiisoft/config`: the project's `configuration.php` lists the
groups and the files in each, and packages contribute their own through
the `config-plugin` of their `composer.json`, which `installed.json`
repeats (`src/yii/definitions.rs` reads the `di` groups from there
already). Read that statically rather than running the merge.

---

## Y9. Views a template renders by name

**Impact: Medium · Complexity: Medium**

```php
// PostController, with withViewPath('@views')
return $this->viewRenderer->render('blog/index');

// resources/views/blog/index.php
<?= $this->render('_card') ?>
```

A view a template renders by name (`$this->render('_card')`) is relative
to the view path of the renderer that rendered the template, not to the
template's own directory. PHPantom resolves it against the directory,
which is the same thing whenever the template was rendered by a bare
name, the common case. When it was rendered as `blog/index` from
`@views`, Yii looks for `@views/_card`, while PHPantom finds
`@views/blog/_card`. Resolve the name against the view paths of the
calls that render the template, the same reverse lookup the lens on a
template makes, and fall back to its directory when nothing renders it.

---

## Y10. The renderer's configured view path

**Impact: Medium · Complexity: Medium** (builds on Y6)

```php
// config/common/params.php
'yiisoft/yii-view-renderer' => [
    'viewPath' => '@views',
],
```

A class that never calls `withViewPath()` renders against the view path
the DI configuration gives the renderer, from
`params['yiisoft/yii-view-renderer']['viewPath']`. PHPantom does not read
it, so a name such a class renders does not navigate, complete, or get
reported. Read the value from the params the same way Y6 does, and use
it as the base wherever the class gives none.

---

## Y7. Render parameters against the template's `@var` list

**Impact: Medium · Complexity: Medium-High**

```php
// Action.php
return $this->viewRenderer->render(__DIR__ . '/template', ['form' => $form]);

// template.php
/**
 * @var WebView $this
 * @var UrlGenerator $urlGenerator
 * @var Form $form
 */
```

Yii templates declare the variables they expect in a `@var` block at the
top, and `yiisoft/demo-blog` does this in every template. That block is
the template's signature, so the `render()` call can be checked against
it: complete the parameter array's keys from it, navigate from a key to
its `@var`, report a value whose type does not match, and report a key
the template does not declare.

A declared variable that the call does not pass is not always an error:
the view's shared parameters (`WebView::setParameters()` in the DI
config, or `parameters` under `params['yiisoft/view']`) and the
renderer's injections (`CsrfViewInjection` gives `$csrf`) supply the
rest, and the layout receives its own. Collect those before reporting a
missing one.

---

## Y8. Column names in query arrays

**Impact: Low-Medium · Complexity: High**

```php
$query->from(Table::POST . ' p')
    ->where(['p.status' => PostStatus::Published])
    ->orderBy(['p.publication_date' => SORT_DESC]);
```

`yiisoft/db` queries name columns as array keys and strings, often
qualified by a table alias the same query declares, and the table
itself is often a class constant with the alias appended (`'post p'`,
or `['p' => 'post']`). The columns come
from the project's migrations (`yiisoft/db-migration`'s
`createTable()` and `addColumn()`). Complete and navigate column names,
and report one no migration creates. Laravel's migration scanning
(`src/virtual_members/laravel/database_schema/`) is the precedent for
reading the schema statically. Wait for a user to ask: the demo is the
only project known to query this way.
