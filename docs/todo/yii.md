# PHPantom — Yii

Known gaps and missing features in PHPantom's Yii support. Today Yii
gets only the generic engine. Yii 3 is built from small, typed
components with little magic, so the engine already types most of it;
what is missing is the navigation between the names Yii keeps in
configuration and the code and files they point to.

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

## Y2. Route names and their handlers

**Impact: High · Complexity: Medium**

```php
// config/common/routes.php
Route::get('/blog/{slug}')
    ->action([PostController::class, 'view'])
    ->name('blog/post'),

// anywhere
$urlGenerator->generate('blog/post', ['slug' => $post->getSlug()]);
```

Navigate from a route name in `UrlGeneratorInterface::generate()` (and
the other URL-building calls) to the route that declares it, and from
the route to its handler: the `[Controller::class, 'method']` array or
the class an `action()` names. Find references on a route name lists
every place that generates it, and on a handler method lists the routes
that point at it. Complete route names and the parameter keys their
path declares, and report a name nothing declares. Account for the name
prefixes and path prefixes a `Group` adds.

The parameter keys are the same problem as Symfony's
[F27](lsp-features.md#f27-symfony-route-parameters); share the design.

Two shapes from `yiisoft/demo-blog` to cover:

- **Route arguments in handlers.** `#[RouteArgument('id')]` on an action
  parameter (`yiisoft/input-http`) names a placeholder in the route's
  path. Navigate to the `{id}`, complete it from the path of the route
  whose handler this is, and report a name the path does not declare.
  Optional segments (`/blog[/{page}]`) declare placeholders too.
- **A wrapper around the URL generator.** The demo extends Yii's URL
  generator with one method per route that calls
  `$this->generate('blog/post/view', …)`. Recognise the call by the
  method it resolves to (any subclass or implementation of
  `UrlGeneratorInterface::generate()`), not by the receiver's spelling.

**Where to look:** the route-name support for Laravel and Symfony
(`src/symbol_map/extraction/symfony.rs`, `src/symfony/routes.rs`).

---

## Y4. DI container configuration

**Impact: Medium-High · Complexity: Medium-High**

```php
// config/common/di/mailer.php
return [
    MailerInterface::class => [
        'class' => Mailer::class,
        '__construct()' => ['transport' => Reference::to(TransportInterface::class)],
        'setLogger()' => [Reference::to(LoggerInterface::class)],
    ],
];
```

`yiisoft/di` definitions are PHP arrays keyed by the ID (usually an
interface) with string keys that name constructor arguments
(`'__construct()'`), methods to call (`'setLogger()'`), and properties
to set (`'$name'`). Make those strings navigate, complete, rename with
the member they name, and report a name the class does not have.
Navigate from an interface to the definition that binds it, and from
`$container->get(Foo::class)` or a `Reference::to()` to the definition.

The same syntax (`yiisoft/definitions`) appears outside the `di` group,
and `yiisoft/demo-blog` uses every one of these:

- the `widgets` config group, which configures every instance of a
  widget class (`GridView::class => ['urlCreator()' => […]]`);
- definitions written inline in PHP, such as a service provider's
  `getDefinitions()` or a static `definition()` helper returning
  `['class' => self::class, '__construct()' => […]]`;
- `'class' => …` entries nested inside another definition's arguments.

Recognise a definition by its shape (a `class` key or a class-string
key, with `'name()'` and `'$name'` keys) wherever the array is written,
not only in files the `di` group lists.

The config files are found through `yiisoft/config`: the project's
`configuration.php` lists the groups (`di`, `di-web`, `params`, …) and
the files in each, and packages contribute their own through
`composer.json`. Read that statically rather than running the merge.

**Where to look:** the Symfony container support (`src/symfony/container.rs`)
for the navigation and rename side.

---

## Y5. Alias definitions

**Impact: Medium · Complexity: Low-Medium**

```php
// config/common/aliases.php, required as params['yiisoft/aliases']['aliases']
return [
    '@root' => dirname(__DIR__, 2),
    '@views' => '@root/resources/views',
];

$aliases->get('@runtime/logs');
public ?string $sourcePath = '@assetsSource/main';   // an asset bundle
```

An alias (`yiisoft/aliases`) names a path in a string starting with
`@`, and aliases are defined in terms of each other. Navigate from an
alias in any string to its definition, complete alias names, and,
where the definition resolves to a path in the project, navigate from
the whole string to the file or directory it names.

View names already resolve their `@views/...` paths through
`src/yii/aliases.rs`, which reads the aliases from every params file
under `config/` that names `yiisoft/aliases`. It records only the path
each alias resolves to, so this item adds where each one is declared.

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
comes from the same `configuration.php` reading Y4 needs.

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
