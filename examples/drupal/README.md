# Drupal Demo Project for PHPantom LSP

A standalone editor playground for PHPantom on Drupal 11. It is a
`drupal/recommended-project` site with one custom module, `acme`, and one
theme, `acme_theme`, declaring a service, a route, a content entity type,
hooks, and a theme hook the way a real Drupal module declares them.

Run `composer install` here first. Composer puts core and contrib into
`web/`, which is git-ignored apart from the custom module and theme.

## What to try

- Open `web/modules/custom/acme/src/Controller/GreetingController.php`. Go to
  the definition of `$this->greeter->greet()`, which lands in the `Greeter`
  service, or complete after `$this->` to see the helpers `ControllerBase`
  inherits from core.
- In `web/modules/custom/acme/src/Greeter.php`, complete after
  `$this->configFactory->`. Core's interfaces type the injected services.
- In `web/modules/custom/acme/src/Entity/Note.php`, complete after
  `BaseFieldDefinition::create('string')->`. Each setter returns the
  definition, so the chain keeps completing.
- In `web/modules/custom/acme/acme.module`, complete after `$route_match->`
  inside the procedural `acme_help()` hook.
- Service IDs and container parameters navigate between the services files
  and the code that looks them up, core's included. In
  `web/modules/custom/acme/acme.module`, hover `$greeter`, typed by the class
  `acme.services.yml` declares for `acme.greeter`, or go to the definition of
  `'acme.greeter'`. In `web/modules/custom/acme/src/NoteCounter.php`, go to
  the definition of `'entity_type.manager'`, which lands in core's
  `core.services.yml`. Complete an ID inside `\Drupal::service('')`,
  `$container->get('')`, or after `@` in `acme.services.yml`, and find the
  references of a service from its declaration.
- Entity type IDs navigate between the entity class that declares them and
  the code that asks the entity type manager for them, and type what that
  code gets back. In `web/modules/custom/acme/src/NoteCounter.php`, hover
  `$storage` and `$note` in `first()`, typed as the storage and as `Note`,
  or go to the definition of `'acme_note'`, which lands on the `id` in
  `Note`'s attribute. Complete an ID inside `getStorage('')`; core's entity
  types are offered too. In `ids()` and `count()`, hover `$ids` and
  `$count`: an entity query gives the IDs of its entity type, or their
  number once it counts, and the ID inside `\Drupal::entityQuery('')`
  navigates and completes too.
- Route names navigate between the routing file that declares them and the
  code, links files, and templates that name them. In
  `web/modules/custom/acme/src/Controller/GreetingController.php`, go to the
  definition of `'acme.greeting'` in `greetTheWorld()` or `worldLink()`,
  which lands on its key in `acme.routing.yml`, or find its references from
  there, `acme.links.menu.yml` and the `path()` in
  `templates/acme-greeting.html.twig` included. Complete a route name inside
  `redirect('')`, `Url::fromRoute('')`, or after `route_name:` in a links
  file; core's routes are offered too. A route name nothing declares is
  reported, except one of the shapes core builds routes in: in
  `web/modules/custom/acme/src/CoreLinks.php`, `'entity.node.canonical'`,
  which core's entity route provider builds, and `'view.content.page_1'`,
  which Views builds for the content view's page, are not.
- Route parameters navigate from the keys that pass them to the placeholder
  of the route's path. In `worldLink()`, go to the definition of `'name'`,
  which lands on `{name}` in `acme.routing.yml`, or complete a key inside the
  parameters array; the `route_parameters:` of `acme.links.menu.yml` and the
  `path()` hash in `templates/acme-greeting.html.twig` go there too.
- A controller's parameter is typed as the entity Drupal upcasts it to. In
  `web/modules/custom/acme/src/Controller/NoteController.php`, hover
  `$acme_note` in `view()`, named after the `acme_note` entity type and
  hinted with an interface `Note` implements, or in the untyped `title()`,
  the same route's title callback: both are a `Note`. A controller hint
  unrelated to the entity class, which leaves Drupal passing the raw ID, is
  reported.
- Hooks navigate between their documentation, the calls that fire them, and
  their implementations. In `web/modules/custom/acme/acme.module`, hover
  `$form_state` in `acme_form_alter()`, typed by core's documentation,
  hover the function's name to read `hook_form_alter()`'s documentation, or go
  to the definition of `form_alter` in the function's name, which lists the
  `alter()` call in core's `FormBuilder` and `hook_form_alter()`. In
  `src/Greeter.php`, go to the definition of `'acme_greeting'`, which lists
  the implementation in `src/Hook/AcmeHooks.php` and the documentation in
  `acme.api.php`. Type `function acme_` on a new line of `acme.module` to
  complete a hook with its documented signature.
- Permission names navigate between the permissions file that declares
  them and the code and routes that require them. In
  `web/modules/custom/acme/src/NoteAccess.php`, hover `'administer acme'` to
  read its title and description from `acme.permissions.yml`, go to the
  definition of `'access content'`, which lands in core's
  `system.permissions.yml`, or complete a permission inside
  `hasPermission('')`. Each part of the `_permission` of `acme.note` in
  `acme.routing.yml` and the `admin_permission` of `Note` navigate and
  complete too.
- Configuration names navigate to the default configuration that ships them
  and the schema entry that describes them. In
  `web/modules/custom/acme/src/Form/SettingsForm.php`, go to the definition
  of `'acme.settings'` in `getEditableConfigNames()` or in the
  `#config_target`, which lists `config/install/acme.settings.yml` and the
  entry in `config/schema/acme.schema.yml`, or complete a configuration name
  inside `['']`. In `web/modules/custom/acme/src/Greeter.php`, go to the
  definition of `'system.site'`, which core ships.
- A module's service provider may build a service from a class of its own.
  In `acme_french_site_name()` in `web/modules/custom/acme/acme.module`,
  hover `$languageManager`: core declares `language_manager` as
  `LanguageManager`, and the language module's `LanguageServiceProvider`
  swaps in `ConfigurableLanguageManager`, so it is either class.

## What the Symfony support already reads

Drupal declares some things in shapes PHPantom already reads for any
project. What works today:

- **Class names and `Class::method` strings in YAML.** In
  `web/modules/custom/acme/acme.routing.yml`, the `_controller` string goes
  to `GreetingController` and to its `greet()` method, and in
  `web/modules/custom/acme/acme.services.yml`, `class:` goes to `Greeter`.
  Each counts among the references of the class or method it names.

What does not work yet:

- **Event subscribers.** The `getSubscribedEvents()` map of an event
  subscriber is read only in a Symfony project, so in a Drupal site its
  event names and listener methods neither complete nor navigate. See
  `docs/todo/drupal.md`.

## Runtime assertions

`assertions.php` reads the module's declarations with Drupal's own loaders
and checks that every name the demo declares, and every claim its "Try:"
comments make, holds at runtime:

```bash
php examples/drupal/assertions.php
```
