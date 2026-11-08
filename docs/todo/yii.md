# PHPantom — Yii

Known gaps and missing features in PHPantom's Yii support. Yii 3 is
built from small, typed components with little magic, so the generic
engine already types most of it. On top of that, route names, route
arguments, view names, and the members and arguments DI definitions name
complete, navigate, and are checked, path aliases navigate to their
declarations, and the `$params` keys a configuration file reads complete,
navigate, are checked, and are typed. What is still missing is the
navigation between the rest of the names Yii keeps in configuration
(events, translations) and the code and files they point to, and checking
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

## Y11. Check what a DI definition passes, not only what it names

**Impact: Medium · Complexity: Medium**

```php
// config/common/di/logger.php
LoggerInterface::class => [
    'class' => Logger::class,
    'alert' => '',                             // neither 'alert()' nor '$alert'
    '$targets' => [],                          // Logger::$targets is private
    'setFlushInterval()' => ['not-an-int'],    // takes an int
],
```

A definition's `'name()'` and `'$name'` keys navigate and are reported
when the class has no such member, but nothing else about them is
checked. `analyze` on the playground with the lines above reports
nothing, while `yiisoft/definitions` throws for each of them when the
container builds the object. Report:

- a key that is not `class`, `__construct()`, or one of the two member
  forms, with a quick fix that adds the `()` or `$` the member it
  matches needs;
- a `'$name'` key whose property is not public, and a `'name()'` key
  whose method is not public;
- a value whose type does not match the property, or an argument whose
  type does not match the parameter it is passed to, positionally or by
  name. The argument check is the one a call gets, so feed the
  definition's arguments through it rather than writing a second one.
  A `Reference`, `DynamicReference`, or `ReferencesArray` value is typed
  as the class or classes it names.

---

## Y13. Translation message IDs

**Impact: Medium · Complexity: Medium**

```php
$translator->translate('menu.blog', category: 'app');

// resources/messages/en/app.php
return ['menu.blog' => 'Blog'];
```

`yiisoft/translator` looks a message ID up in the category's message
source, which for `yiisoft/translator-message-php` is a PHP file per
locale and category returning an array. Complete the IDs the category's
files declare, navigate from an ID to its entry in each locale, show the
default locale's text on hover, and report an ID no file declares. The
category is the `category` argument, else the translator's default
(`withDefaultCategory()` or the DI configuration). The sources and their
directories come from the translator's DI definitions, which `src/yii/definitions.rs`
already reads. `yiisoft/demo-blog` does not
translate, so test against the playground.

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

## Y12. Missing layouts and view directories

**Impact: Low-Medium · Complexity: Low-Medium**

```php
$this->viewRenderer
    ->withViewPath('@views/blogg')     // no such directory
    ->withLayout('@layout/mainn');     // no such file
```

A view a class renders by name is reported when no template matches it,
but the layout and the view path the name is resolved against are not.
Navigate from a `withLayout()` argument to its template and complete it
the way a view name completes, and report a layout with no file and a
`withViewPath()` directory that does not exist. The aliases resolve
through the configuration's declarations, as view names already do.

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

## Y20. `config-plugin` entries in `composer.json`

**Impact: Low · Complexity: Low**

```json
"extra": {
    "config-plugin": {
        "params": "config/params.php",
        "di-web": ["config/di-web.php", "$di"]
    }
}
```

A package's or the project's `config-plugin` maps each config group to
its files. Complete the group names `configuration.php` and the
installed packages already use, and the paths to the project's files,
navigate from a path to its file and from a `$group` reference to that
group's entries, and report a path with no file.

---

## Y16. Asset bundle file paths

**Impact: Low · Complexity: Low-Medium**

```php
final class MainAsset extends AssetBundle
{
    public ?string $sourcePath = '@assetsSource/main';
    public array $css = ['site.css'];
}
```

The aliases in `$basePath`, `$baseUrl`, and `$sourcePath` already
navigate. The entries of `$css` and `$js` are relative to `$sourcePath`
(or `$basePath` when it is unset) and do not. Navigate from an entry to
its file, complete from the files under that directory, and report an
entry with no file. An entry that is a URL is left alone.

---

## Y19. Aliases in workspace symbols and folding

**Impact: Low · Complexity: Low-Medium**

The aliases the configuration declares navigate, complete, and show
their path on hover, but cannot be found by name. List each declaration
in workspace symbols (`@views` finds the line in `aliases.php` that sets
it), and, for clients that show folded text, offer a folding range over
an aliased path whose collapsed text is the path it resolves to.

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

---

## Y21. CSS class names in HTML helpers

**Impact: Low · Complexity: Medium-High**

```php
Html::div('...')->class('card-body');
```

`yiisoft/html` builds tags with `class()` and `addClass()`. Completing
and navigating class names there needs an index of the project's
stylesheets, which PHPantom does not have for any framework. File the
stylesheet index as its own item first if this is ever taken up.
