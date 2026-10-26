# PHPantom — Drupal

Known gaps and missing features in PHPantom's Drupal support. Today
Drupal support is project discovery only: the web root is detected, and
core, contrib, and the Drupal file extensions (`.module`, `.install`,
`.theme`, …) are indexed past `.gitignore`. Nothing reads Drupal's own
declarations: services, entity types, fields, routes, or hooks.

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

## DR1. Drupal demo project and a real-world baseline

**Impact: High · Complexity: Low-Medium**

Every other Drupal item is measured against this one. Without a demo
project and a baseline from real code, Drupal support would be built
untested and could regress silently.

- **`examples/drupal/`**: a `drupal/recommended-project` site with one
  custom module and one theme, in the same shape as `examples/laravel/`
  and `examples/symfony/`, with "Try:" comments at each site and a
  README listing what to try. The module declares a service, a route,
  a content entity type, a hook implementation (procedural and
  `#[Hook]`), and a render array with a theme hook.
- **Runtime assertions**: an `assertions.php` that checks the demo's
  claims against real Drupal. Decide whether that bootstraps a kernel
  (core supports SQLite, so no server is needed) or only checks what
  can be read without booting.
- **A real-world baseline**: run `analyze` on an actively maintained
  contrib module and a site build, bucket the diagnostics by cause, and
  file what Drupal-specific gaps they show. That ranks the rest of this
  document by what actually breaks.

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
including `@service_id` arguments in the YAML.

The Symfony container reading already parses this format and follows
aliases and parameter placeholders. Start by checking what it picks up from a
Drupal module unchanged, then teach it where Drupal keeps the files
(every enabled module's and core's `*.services.yml`) instead of
`config/services.yaml`.

**Where to look:** `src/symfony/container.rs`, `src/symfony/resources.rs`,
and PHPStan's Drupal extension (its service map).

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
them, and complete and navigate entity type IDs.

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

The Twig support already finds templates outside Symfony; the
template's variables are the same problem as
[F37](lsp-features.md#f37-twig-template-variables).

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
