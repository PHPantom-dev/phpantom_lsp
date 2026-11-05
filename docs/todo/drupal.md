# PHPantom — Drupal

Known gaps and missing features in PHPantom's Drupal support. Today
Drupal support is project discovery only: the web root is detected, and
core, contrib, and the Drupal file extensions (`.module`, `.install`,
`.theme`, …) are indexed past `.gitignore`. Nothing reads Drupal's own
declarations: services, entity types, fields, routes, or hooks.

The requests in
[#48](https://github.com/PHPantom-dev/phpantom_lsp/issues/48) are the
scope of this document: the indexing it asked for first has shipped, and
every Drupal-specific feature its discussion lists is one of the items
below.

Drupal is built on Symfony components, so its service container and
YAML files are close to what the Symfony support already reads. PHPStan's
Drupal extension (`mglaman/phpstan-drupal`) is the checklist of what a
Drupal developer expects from types; PhpStorm's built-in Drupal support
is what they expect from an editor.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## DR2. Service IDs from `*.services.yml`

**Impact: High · Complexity: Medium**

```php
$manager = \Drupal::service('acme.order_manager');

public static function create(ContainerInterface $container): static {
    return new static($container->get('entity_type.manager'));
}
```

A module declares its services in `<module>.services.yml`, in Symfony's
service format. Type `\Drupal::service()` and `$container->get()` by the
class the declaration names, and complete and navigate the service IDs,
including `@service_id` arguments in the YAML. Newer code registers a
service under its interface and fetches it with
`\Drupal::service(OrderManagerInterface::class)`; that ID is a class
name and types as that class without any lookup.

The Symfony container reading already parses this format and follows
aliases and parameter placeholders. Start by checking what it picks up from a
Drupal module unchanged, then teach it where Drupal keeps the files
(every enabled module's and core's `*.services.yml`) instead of
`config/services.yaml`.

Most of Drupal's navigation depends on this. On Drupal core, Find
References on `EntityStorageInterface::load()` misses about 240 calls
of the shape
`$this->container->get('entity_type.manager')->getStorage('node')->load(1)`.
They are the bulk of what a tool that types service lookups finds and
we don't.

**Where to look:** `src/symfony/container.rs`, `src/symfony/resources.rs`,
PHPStan's Drupal extension (its service map), and PHPlumb
(`crates/phplumb-index/src/frameworks/drupal/services.rs`, MIT), which
reads the same files.

---

## DR3. Entity storage and entity type IDs

**Impact: High · Complexity: Medium-High**

```php
$storage = \Drupal::entityTypeManager()->getStorage('node');  // NodeStorageInterface
$node = $storage->load($nid);                                  // ?NodeInterface
```

`getStorage()` returns the generic `EntityStorageInterface`, so every
loaded entity is the generic `EntityInterface` and the methods a node
or user has are reported as unknown. The entity type ID maps to an
entity class and its storage handler through the entity type's
declaration: the `#[ContentEntityType]` / `#[ConfigEntityType]`
attribute, or the annotation older code uses. Read those statically,
type `getStorage()`, `load()`, `loadMultiple()`, and `create()` from
them, and complete and navigate entity type IDs. Entity queries
(`\Drupal::entityQuery('node')`, `$storage->getQuery()`) take the same
IDs and return IDs of that entity type.

**Where to look:** PHPStan's Drupal extension (its entity data
repository and the storage return-type extensions).

---

## DR4. Entity field access

**Impact: Medium-High · Complexity: High** (depends on DR3)

```php
$tags = $node->field_tags;          // FieldItemListInterface
$title = $node->get('title')->value;
```

Fields are reached through magic properties and `get()`. A field name
comes from the entity class's `baseFieldDefinitions()` or from
configuration (`field.storage.<entity>.<field>.yml` in the exported
config or a module's `config/install`). Type a field access as the field
item list its field type gives (an entity reference field gives
`EntityReferenceFieldItemListInterface`), and complete and navigate
field names.

PHPStan's Drupal extension types any magic property on a fieldable
entity as `FieldItemListInterface` without checking the name. That is
the floor; reading the configuration is what lets an editor complete
names and report a misspelt one.

---

## DR5. Route names

**Impact: Medium-High · Complexity: Medium**

```php
Url::fromRoute('acme.order_view', ['order' => $id]);
$form_state->setRedirect('acme.order_list');
```

```yaml
acme.order_view:
  path: '/order/{order}'
  defaults:
    _controller: '\Drupal\acme\Controller\OrderController::view'
```

Routes are declared in `<module>.routing.yml`. Complete, navigate, and
find references for route names in `Url::fromRoute()`,
`Link::createFromRoute()`, `$this->redirect()`,
`$form_state->setRedirect()`, and the menu, tab, and action YAML files,
and report a name nothing declares. The `_controller`, `_form`, and
`_entity_form` values may already navigate through the resource symbol
maps that read `Class::method` strings in YAML; check before building
anything for them. The parameter keys are the same problem as Symfony's
[F27](lsp-features.md#f27-symfony-route-parameters).

A path parameter named after an entity type (`/node/{node}`) is upcast
by Drupal's parameter converters, so the controller method's `$node`
receives a `NodeInterface`, not the ID. Type an untyped controller
parameter from the route that names it, and check a typed one against
it.

---

## DR6. Hooks

**Impact: Medium-High · Complexity: Medium**

```php
function acme_form_alter(&$form, FormStateInterface $form_state, $form_id) { … }

#[Hook('form_alter')]
public function formAlter(array &$form, FormStateInterface $form_state, $form_id): void { … }
```

A hook is implemented by naming a function `<module>_<hook>` or, since
Drupal 11.1, with a `#[Hook]` attribute on a method. Core and modules
document each hook as a `hook_<name>()` function in a `*.api.php` file.
Link an implementation to that documentation: hover shows it,
go-to-definition reaches it, find references lists the implementations,
and completing `acme_` in a `.module` file offers the hooks with their
documented signatures. Parameter types for an implementation that
omits them can come from the documented signature.

---

## DR7. Render arrays, theme hooks, and libraries

**Impact: Medium · Complexity: Medium-High**

```php
return [
    '#theme' => 'acme_order_summary',
    '#order' => $order,
    '#attached' => ['library' => ['acme/order-summary']],
];
```

- **`#theme`** names a theme hook a `hook_theme()` implementation
  declares, which renders `templates/acme-order-summary.html.twig`.
  Navigate from the string to both, complete it, and complete the
  `#`-prefixed variables the hook declares.
- **`#type`** names a render or form element class
  (`#[RenderElement('details')]`, `#[FormElement('textfield')]`).
- **Libraries** in `#attached` name entries in `<module>.libraries.yml`.

The Twig support already finds templates outside Symfony, and a
template's variables from the renders that name it; a theme hook's
variables would be one more source of them (see `src/twig/variables.rs`).

---

## DR8. Permission and configuration names

**Impact: Low-Medium · Complexity: Medium**

```php
$account->hasPermission('administer acme orders');
\Drupal::config('acme.settings')->get('notify_email');
```

Permissions are declared in `<module>.permissions.yml` (and by
permission callbacks), configuration objects in `config/install` and
`config/schema`. Complete and navigate both, and complete the keys
`get()` reads from the schema.

---

## DR9. Drupal directories outside `contrib` and `custom`

**Impact: Medium-High · Complexity: Low-Medium**

The scan that bypasses `.gitignore` covers `core`, `modules/contrib`,
`modules/custom`, `themes/contrib`, `themes/custom`, `profiles`, and
`sites` under the web root. Sites also group modules by vendor
(`modules/acme/`) or keep them directly under `modules/`, and profiles
carry their own `modules/` and `themes/`; a module a site's `.gitignore`
hides in any of those places is not indexed, and its classes read as
unknown. Scan `modules`, `themes`, and `profiles` whole.

The scan also runs only when the Drupal site is the workspace's Composer
root. A workspace that holds the site beside other Composer projects, or
opens a Drupal tree without a `composer.json`, gets none of it. Run it
for each Composer project in the workspace that requires `drupal/core`,
and give the scanned classes the same origin (project, vendor) other
classes get, so completion ranks and filters them the same way.

**Where to look:** `scan_drupal_directories` in
`src/classmap_scanner/discovery.rs`, its caller in
`src/indexing/init.rs`, and how per-project Composer roots are handled
there.

---

## DR10. Plugin IDs

**Impact: Medium · Complexity: Medium**

```php
#[Block(id: 'acme_order_summary', admin_label: new TranslatableMarkup('Order summary'))]
final class OrderSummaryBlock extends BlockBase { … }

$block = $this->blockManager->createInstance('acme_order_summary');
```

Blocks, field types, formatters, widgets, queue workers, and the other
plugin types declare an ID in an attribute (or, in older code, a
docblock annotation). Complete and navigate plugin IDs where a plugin
manager takes one (`createInstance()`, `getDefinition()`,
`hasDefinition()`), and in the YAML and render arrays that name them
(`'#type'` is [DR7](#dr7-render-arrays-theme-hooks-and-libraries)), and
type `createInstance()` as the plugin class.

---

## DR11. Form and render callbacks

**Impact: Low-Medium · Complexity: Low-Medium**

```php
$form['actions']['submit']['#submit'][] = '::submitOrder';
$form['country']['#ajax'] = ['callback' => '::updateRegions'];
```

`#submit`, `#validate`, `#element_validate`, `#pre_render`,
`#after_build`, and an `#ajax` `callback` name methods of the form class
as `'::method'`, or a function or `Class::method`. Navigate, rename, and
complete them, and report one that names nothing.

---

## DR12. Translation placeholders

**Impact: Low-Medium · Complexity: Low-Medium**

```php
$this->t('Order @number was placed by %name', ['@number' => $id, '%nmae' => $name]);
```

`t()`, `$this->t()`, `new TranslatableMarkup()`, and `formatPlural()`
replace `@`, `%`, and `:` placeholders from the array they are given.
Report a placeholder the string uses that the array does not supply, and
a key the string never uses.

---

## DR13. Module names and `*.info.yml`

**Impact: Low-Medium · Complexity: Low-Medium**

```php
\Drupal::moduleHandler()->moduleExists('acme_shop');
```

```yaml
dependencies:
  - drupal:node
  - acme:acme_shop
```

A module's machine name is the basename of its `*.info.yml`. Complete
and navigate machine names in `moduleExists()`, `getModule()`, and the
`dependencies` of an info file, and report one that names no module.
Check an info file's required keys (`name`, `type`,
`core_version_requirement`) and their shapes.

---

## DR14. Port PHPStan's Drupal extension tests

**Impact: Medium · Complexity: Medium**

PHPStan's Drupal extension (`mglaman/phpstan-drupal`) is this
document's checklist, but its tests are not ported, as the Symfony,
Doctrine, and WordPress extensions' are (`tests/phpstan_symfony/`,
`tests/phpstan_doctrine/`, `tests/wordpress_stubs/`). Its type tests
cover `\Drupal::service()` and container lookups, entity storage and
entity queries, field item lists, and the rule tests deprecated and
discouraged calls. Port them into `tests/phpstan_drupal/` the same way:
the `assertType()` fixtures run by `tests/assert_type_runner.rs`, the
rule tests as `.fixture` files, a slice of `drupal/core` copied by
`scripts/extract-framework-stubs.php`, and upstream's test modules and
their `*.services.yml` as the project. Until DR2 to DR4 ship most
assertions are a `// SKIP` pointing at the item that covers them; file
any that no item does. Rules about Drupal's own deprecations are listed
in the README as not ported.

---

## DR15. Benchmark for Drupal names

**Impact: Low-Medium · Complexity: Low** (depends on DR2)

Laravel, Symfony, and Yii have Criterion benchmarks in `benches/`.
Once Drupal service IDs complete (DR2), add
`benches/drupal_completion.rs` in the shape of
`benches/symfony_completion.rs`: generate a site with many modules,
each declaring services, routes, and hooks, and time completion of
service IDs, route names, and hook names as each ships. Register it in
`Cargo.toml` so `benches/ci_benchmarks.py` picks it up.

---

## DR16. Event subscribers

**Impact: Medium · Complexity: Low-Medium**

```php
final class AcmeSubscriber implements EventSubscriberInterface {
    public static function getSubscribedEvents(): array {
        return [KernelEvents::REQUEST => 'onRequest'];
    }
}
```

Drupal's event subscribers are Symfony's: a service tagged
`event_subscriber` maps event names to its own methods in
`getSubscribedEvents()`. The Symfony support reads that map, jumps from
a listener name to its method, and reports one that does not exist,
but only in a Symfony project, and a Drupal site's `composer.json`
requires none of the packages that make it one. Read the map in a
Drupal site too, so the listener methods navigate and are checked, and
the event names complete where Drupal's event dispatcher takes one.

---

## DR21. Return types core's docblocks leave wide

**Impact: Medium · Complexity: Low-Medium**

```php
$url = Url::fromRoute('acme.greeting')->toString();           // string|GeneratedUrl
$query = $db->select('node', 'n')->extend(PagerSelectExtender::class);
$query->limit(10);                                            // limit() not found on ExtendableInterface
```

A few core methods that Drupal code calls everywhere declare a return
type wider than what the call gives. `Url::toString()` returns a
`GeneratedUrl` only when `$collect_bubbleable_metadata` is `true`, and a
`string` otherwise; PHPStan's Drupal extension ships a stub with the
conditional return type `($collect_bubbleable_metadata is true ?
\Drupal\Core\GeneratedUrl : string)`. `extend()` on a query returns an
instance of the extender class it is given, but declares
`ExtendableInterface`, so every pager or table-sort query loses its
type. Give both the precise type, through the same mechanism as the
other framework corrections ([E7](external-stubs.md#e7-stub-based-framework-patches)
if it has shipped), so a `string` parameter takes the plain
`toString()` and the extender's methods resolve.
