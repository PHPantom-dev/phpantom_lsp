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
Symfony scenarios. There is no completion benchmark yet because Yii has
no name completion to measure: the first item that adds one (Y2, Y3,
Y5, or Y6) creates `benches/yii_completion.rs`, modelled on
`benches/symfony_completion.rs` (a generated mid-sized application with
a case per kind of name), wires it into the `cargo bench` lists in
`.github/workflows/ci.yml` and `docs/CONTRIBUTING.md`, and every later
one adds its case to it.

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

## Y3. View names and their templates

**Impact: High · Complexity: Medium**

```php
return $this->viewRenderer->render('index', ['posts' => $posts]);

return $this->viewRenderer->render(__DIR__ . '/template', ['form' => $form]);
```

A view is named one of two ways, and both are common:

- **By name**, resolved against the renderer's view path, set with
  `withViewPath()` or derived from the controller (`withController()`),
  usually an `@views/...` alias (Y5). This is the `yiisoft/app` style.
- **By path**, `__DIR__ . '/template'`, a template next to the action
  class. The renderer adds the `.php` extension, so the path never names
  the file exactly. `yiisoft/demo-blog` renders every view this way, and
  apps that render through a wrapper of their own
  (`$this->responseFactory->render(...)`) still pass the path through.

Navigate from either form to the template file and back from a
template to the calls that render it, complete view names from the
template directory, and report a literal name or path no file matches.
A view rendered from inside a template (`$this->render('_form', …)`)
resolves against the same view path, and one starting with `./` or
`../` against the template's own directory. The include links in
`src/document_links.rs` already resolve `__DIR__ . '/x.php'` for
`require`; the path form is the same evaluation without the extension.

**Where to look:** Laravel's view names
(`src/virtual_members/laravel/view_names.rs`) and the Twig template-name
support. Check the renderer's current class and method names in
`yiisoft/yii-view-renderer` first, since the package has renamed them.

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
the whole string to the file or directory it names. Y3 builds on this
for `@views/...` view paths.

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

## Y7. Render parameters against the template's `@var` list

**Impact: Medium · Complexity: Medium-High** (depends on Y3)

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
