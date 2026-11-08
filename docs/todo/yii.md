# PHPantom — Yii

Known gaps and missing features in PHPantom's Yii support. Yii 3 is
built from small, typed components with little magic, so the generic
engine already types most of it. On top of that, route names, route
arguments, view names, and the members and arguments DI definitions name
complete, navigate, and are checked, path aliases navigate to their
declarations, the files an asset bundle lists navigate, complete, and are
checked, the `$params` keys a configuration file reads complete,
navigate, are checked, and are typed, and the message IDs a translator
looks up complete, navigate, and are checked. What is still missing is
the navigation between the rest of the names Yii keeps in configuration
(events) and the code and files they point to, and checking
the values a DI definition passes rather than only the names it uses.

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

**Impact: Medium · Complexity: Medium**

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
reported. Read the value from the merged params (`src/yii/params.rs`),
and use it as the base wherever the class gives none.

---

## Y14. Event listeners in the events configuration

**Impact: Medium · Complexity: Medium**

```php
// config/common/events.php
return [
    UserRegistered::class => [
        [WelcomeMailer::class, 'send'],
        static fn (UserRegistered $event) => ...,
    ],
];
```

`yiisoft/yii-event` reads listeners from the `events`, `events-web`, and
`events-console` groups. The method string of an `[Handler::class,
'method']` listener should navigate, complete from the handler's public
methods, rename with the method, and be reported when the handler has
none of that name, or when its first parameter cannot take the event.
Go-to-implementation and a CodeLens on an event class should list the
listeners the configuration attaches to it, the installed packages'
included. The groups come from `configuration.php` and the packages'
`config-plugin`, the same merge plan `src/yii/params.rs` reads.

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

## Y15. Property names in hydrator attributes and form fields

**Impact: Low-Medium · Complexity: Medium**

```php
final class PostForm extends FormModel
{
    public function __construct(
        #[Data('post_title')] public string $title = '',
    ) {}
}

Field::text($form, 'title');
```

`yiisoft/hydrator`'s `#[Data]` and `#[Map]` name the keys of the data
array a property is filled from, and `yiisoft/form-model`'s field
helpers name a property of the form model they are given. A form field
name should navigate to the property, complete from the model's
properties, rename with it, and be reported when the model has no such
property. A `#[Map]` entry is the same check in the other direction, on
its property side.

---

## Y8. Column names in query arrays

**Impact: Low-Medium · Complexity: High**

```php
$query->from(Table::POST . ' p')
    ->where(['p.status' => PostStatus::Published])
    ->orderBy(['p.publication_date' => SORT_DESC]);
```

`yiisoft/db` queries name tables and columns as strings, and columns as
array keys, often
qualified by a table alias the same query declares, and the table
itself is often a class constant with the alias appended (`'post p'`,
or `['p' => 'post']`). The columns come
from the project's migrations (`yiisoft/db-migration`'s
`createTable()` and `addColumn()`). Complete and navigate table and
column names, and report one no migration creates. Laravel's migration
scanning (`src/virtual_members/laravel/database_schema/`) is the
precedent for reading the schema statically. Wait for a user to ask: the
demo is the only project known to query this way.

---

## Y18. Active Record attributes and relations

**Impact: Low-Medium · Complexity: Medium-High**

```php
$post = Post::query()->with('author')->where(['status' => 1])->one();
$post->get('title');
```

`yiisoft/active-record` names attributes and relations as strings:
`get()` and `set()` take an attribute, `with()` and `relation()` a
relation, whose getter is a `get{Name}Query()` method on the record.
Complete, navigate, and check the relation names against those methods,
and the attribute names against the record's properties, or, for a
record without properties, against the table's columns (Y8). Wait for a
user to ask: no project in our test set uses it.

---

## Y17. Key paths in `yiisoft/arrays`

**Impact: Low · Complexity: Medium**

```php
ArrayHelper::getValue($user, 'profile.address.city');
```

`ArrayHelper::getValue()`, `setValue()`, `keyExists()`, and `remove()`
take a key or a dotted path into the array or object they are given.
When the shape of that value is known, complete each segment from its
keys and properties, navigate from a segment to its declaration, and
type `getValue()` as the value the path reaches instead of `mixed`. Any
dotted-path helper (Laravel's `data_get()` and `Arr::get()` take the
same kind of path) would share the walk, so build it in the shared
resolution path rather than for Yii alone.
