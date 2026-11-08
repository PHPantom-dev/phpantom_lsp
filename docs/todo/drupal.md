# PHPantom — Drupal

Known gaps and missing features in PHPantom's Drupal support. The web
root is detected with or without a `composer.json`. Core and every
module, theme, and profile, with the Drupal file extensions (`.module`,
`.install`, `.theme`, …), are indexed past `.gitignore`, and
the services the `*.services.yml` files declare, the entity types
entity classes declare, the routes the routing files, route attributes,
and route builders declare, the hooks the `*.api.php` files document
and the extensions implement, the plugin types plugin managers declare and
the plugins of each, the permissions the `*.permissions.yml`
files declare, the configuration objects and keys the default
configuration and the configuration schema declare, the theme hooks
`hook_theme()` implementations declare, the render elements element
classes declare, and the libraries the `*.libraries.yml` files declare
are read. Nothing reads fields yet.

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

**Benchmarks.** `benches/memory_usage.py` measures memory on a site that
installs core into `web/core` through Composer (`memory_drupal_module`),
next to the other framework scenarios. `benches/drupal_completion.rs`
measures completion of service IDs, route names, entity type IDs,
permission names, configuration names, render element names, plugin IDs,
hook names, theme hook names, library names, and hook implementations over a
generated site with many custom modules. Every item that adds a name
Drupal declares adds its case to it.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## DR4. Entity field access

**Impact: Medium-High · Complexity: High**

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

## DR17. Warn on unknown service IDs

**Impact: Medium · Complexity: Low-Medium**

```php
$manager = \Drupal::service('acme.order_manger');
```

A service ID no `*.services.yml` declares is a typo or a module the site
lacks, and the container throws for it at run time. Report one where
`\Drupal::service()` and `$container->get()` take it. A module's
`ServiceProvider` class can register services in PHP, which no YAML file
lists, so an ID such a provider may register must not warn.

---

## DR18. Warn on unknown entity type IDs

**Impact: Medium · Complexity: Low-Medium**

```php
$storage = \Drupal::entityTypeManager()->getStorage('nod');
```

An entity type ID no entity class declares is a typo or a module the
site lacks, and the entity type manager throws for it at run time.
Report one where `getStorage()`, `\Drupal::entityQuery()`, and the other
calls that take an entity type ID take it. A module can add entity types
in code from `hook_entity_type_build()`, which no class declares, so a
site where an implementation of that hook may add one stays silent.

---

## DR19. Warn on unknown hook names

**Impact: Low-Medium · Complexity: Medium**

```php
\Drupal::moduleHandler()->invokeAll('acme_order_placd', [$order]);
```

Any module may invoke any hook name, and many hooks have no `*.api.php`
documentation, so a name nothing documents is not wrong on that alone.
Report only a hook name that nothing in the site documents, implements,
or invokes anywhere else, which is most likely a typo.

---

## DR20. Complete route names built in code

**Impact: Low-Medium · Complexity: Medium**

```php
Url::fromRoute('entity.node.canonical', ['node' => $nid]);
Url::fromRoute('view.content.page_1');
```

Entity routes and Views routes are accepted without a warning, but
neither completes nor goes to a definition, since no routing file declares
them. Read them from where Drupal builds them: each entity type's link
templates (`entity.<type>.<link>`) and each view's displays
(`view.<view_id>.<display_id>`).

The same reading would narrow what goes unreported. Any `view.<a>.<b>`
is accepted by its shape alone, so a misspelt view or display is not
reported. And a route builder that adds a route under a name held in a
variable or returned by a call (`$collection->add($route_name, $route)`,
as Views and JSON:API do) leaves every name under its extension
(`views.*`, `jsonapi.*`) unjudged. Following the variable to the string
the method assigns it (`"view.$view_id.$display_id"`) would keep only that
shape.

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

---

## DR22. Complete permissions built in code

**Impact: Low-Medium · Complexity: Medium**

```php
$account->hasPermission('create article content');
$account->hasPermission('use text format basic_html');
```

Permissions a permissions file builds through its
`permission_callbacks` neither complete nor go to a definition, since no
key of the file declares them. Ten of core's permissions files do, among
them node, taxonomy, media, block content, and filter, which build one
set of permissions per bundle or text format. Read them from where
Drupal builds them: each callback returns an array keyed by names such as
`"create $type_id content"`, and the bundles and formats the names are
built from are configuration entities (`node.type.*`,
`filter.format.*`). Their names come from the default configuration,
and on most sites from the exported configuration, which
[DR4](#dr4-entity-field-access) reads. A name built from something the
site's files do not hold could be kept as a pattern, as route builders'
names are.

---

## DR23. Warn on unknown permission names

**Impact: Medium · Complexity: Low-Medium**

```php
$account->hasPermission('administer acme ordres');
$account->hasPermission('administer acme+access content');
```

A permission name no `*.permissions.yml` declares is a typo or a module
the site lacks, and Drupal does not throw for it: the check fails, and the
mistake shows only as access denied. `hasPermission()` checks one exact
name, so `'administer acme+access content'` is a name of its own; `+` and
`,` only combine permissions in a route's `_permission`. Report a name
nothing declares wherever permission names are read: the access checks,
the access result helpers, the user module's role functions, each part of
a route's `_permission`, and an entity type's `admin_permission`. This
depends on [DR22](#dr22-complete-permissions-built-in-code): the names
permission callbacks build would otherwise be reported.

---

## DR24. Warn on unknown theme hook names

**Impact: Low-Medium · Complexity: Medium-High**

```php
return ['#theme' => 'acme_order_sumary', '#order' => $order];
```

A `#theme` that names no theme hook never reaches a template: Drupal
logs a warning and renders only the array's children and `#markup`. Render element and library names are reported when
nothing declares them, but theme hook names are not, because the hooks
read from `hook_theme()` implementations are not all of them. Views, in
core, and some contributed modules build their hooks in a loop or from
other calls, which are not read, and a hook name does not say which
extension declares it, so the only safe rule, silence wherever an
implementation cannot be read, would silence every site. Report a name
only once those hooks are read, or with a rule that knows which names
such an implementation can add. A suggestion such as
`acme_greeting__compact` is valid when its base hook is.

---

## DR25. Warn on unknown plugin IDs

**Impact: Medium · Complexity: Low-Medium**

```php
$fields['total'] = BaseFieldDefinition::create('decimel');
$block = $this->blockManager->createInstance('acme_order_sumary');
```

A plugin ID that no plugin of its type declares is a typo or a module the
site lacks. The plugin manager throws for it, except for widgets and
formatters, which silently fall back to the field type's default. Report
one wherever plugin IDs are read (see `src/drupal/plugins.rs`): field types,
widgets and formatters, constraints, and a plugin manager's
`createInstance()`. A derivative ID (`system_menu_block:main`) is known
only at run time, so an ID whose base plugin has a deriver must not
warn.

---

## DR26. Annotation class names

**Impact: Low-Medium · Complexity: Low-Medium**

```php
/**
 * @Block(
 *   id = "acme_order_summary",
 *   admin_label = @Translation("Order summary"),
 * )
 */
final class OrderSummaryBlock extends BlockBase { … }
```

Plugins written before Drupal 10.2 declare themselves in a docblock
annotation, and many contributed modules still do. `@Block` and
`@Translation` name annotation classes, but Drupal's annotated plugin
discovery finds them without a `use` import: it reads a plugin type's
annotation class and namespaces from its plugin manager, not from the
file. Annotations are read through the imports already; resolve a name
the imports don't cover through the namespaces of the plugin type the
class is discovered for, so that go-to-definition, hover, and references
reach the class.

---

## DR27. Plugin IDs in YAML

**Impact: Low-Medium · Complexity: Medium**

```yaml
# config/install/core.entity_form_display.node.article.default.yml
content:
  title:
    type: string_textfield
# config/install/block.block.acme_theme_greeting.yml
plugin: acme_greeting_block
```

Plugin IDs complete and navigate in PHP: a plugin manager's lookups, the
field type `BaseFieldDefinition::create()` takes, the widget or formatter
of a field's display options, and the constraint `addConstraint()` takes.
The YAML that names them does not yet: a form or view display's `type`
(a widget or formatter), a field storage's `type` (a field type), a
block's `plugin`, and a migration's `source`, `process`, and
`destination` plugins. Default configuration files are not parsed today
(see `drupal::config`), so this needs a reader for the keys that name a
plugin and the manager each one belongs to.

---

## DR28. Plugin types a manager builds at run time

**Impact: Medium · Complexity: Medium**

```php
class ViewsPluginManager extends DefaultPluginManager {
  public function __construct($type, \Traversable $namespaces, …) {
    parent::__construct("Plugin/views/$type", $namespaces, …);
  }
}
```

A plugin type is read from the literals a manager's constructor passes to
`DefaultPluginManager`'s. Views and Migrate serve many plugin types from
one manager class whose subdirectory is built from a constructor
argument, which each service in `views.services.yml` and
`migrate.services.yml` passes (`plugin.manager.views.field` passes
`field`), so their plugins (Views fields, filters, and displays, Migrate
sources and processes) neither complete nor navigate. Read the argument
each such service passes, and type the service by the plugin type it
builds.
