# Drupal Demo Project for PHPantom LSP

A standalone editor playground for PHPantom on Drupal 11. It is a
`drupal/recommended-project` site with one custom module, `acme`, and one
theme, `acme_theme`, declaring a service, a route, a content entity type,
a render element, a block plugin, hooks, a theme hook, permissions, and configuration the
way a real Drupal module declares them.

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
  references of a service from its declaration. Misspell `'acme.greeter'` in
  `acme.module` and the lookup is reported, since no services file declares
  the ID and no service provider defines it.
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
- An entity's base fields are typed properties of its class and of its
  interfaces. In `web/modules/custom/acme/src/NoteSummary.php`, complete
  after `$note->`: `title` comes from `Note`, `id` and `uuid` from core's
  `ContentEntityBase` under the names the entity keys give, and `uid` from
  core's owner trait. Hover `$title`, a list of `StringItem` items, and
  `$author`, an entity reference list that names `User`, or go to the
  definition of `title`, which lands on its `$fields['title']` line in
  `Note`.
- Configurable fields come from field storage files: the site's exported
  configuration in `config/sync`, beside the web root, and the default
  configuration of modules and profiles. In `NoteSummary.php`, `field_tags`
  completes after `$note->` too; hover `$tags`, an entity reference list
  that names `Term`, or go to the definition of `field_tags`, which lands in
  `config/sync/field.storage.acme_note.field_tags.yml`.
- A field reads the properties of its first item, the ones its item class
  declares in `propertyDefinitions()`, inherited ones included. In
  `NoteSummary.php`, complete after `$note->field_tags->` to see
  `target_id` and `entity`; hover `$firstTag`, a `?Term`, and
  `$authorAccount`, a `?User`, the class of the entity type each field
  points to. Every stored value, such as `$titleText`, stays `mixed`.
- A field's name in a fieldable entity's `get()`, `set()`, `hasField()`,
  and `getFieldDefinition()` is one of its fields. In `NoteSummary.php`,
  complete inside `hasField('')` in `isTagged()`, go to the definition of
  `'field_tags'`, which lands in its field storage file, hover it for its
  field type, or find its references, the `$note->field_tags` reads
  included; `$tags`, from `get('field_tags')`, is typed as the field is.
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
- A controller or title callback written as `service:method` names a method
  of the service's class. In `web/modules/custom/acme/acme.routing.yml`, go
  to the definition of `greet` in `acme.greeter:greet`, which lands in
  `Greeter`, complete a method after the `:`, or find the references of
  `Greeter::greet()` to see the routing file among them.
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
- Configuration keys complete and navigate from the schema of the object
  they belong to. In `SettingsForm`, complete a key inside `get('')` or
  `set('')` on `$this->config('acme.settings')`, one level at a time after
  `notifications.`, or after the `:` of the `#config_target`, and go to the
  definition of `'notify_email'`, which lands on its line in
  `acme.schema.yml`. Hover `$settings`, whose type carries the name it was
  read by.
- Theme hooks navigate between the render arrays that name them, their
  `hook_theme()` entry, and their templates, core's included. In
  `GreetingController::greet()`, go to the definition of `'acme_greeting'`,
  which lists its key in `src/Hook/AcmeHooks.php`, the module's
  `templates/acme-greeting.html.twig`, and `acme_theme`'s override, or hover
  it to see its template and variables. Complete a `#theme` value, core's
  `item_list` included, or a key after `'#'` to see the hook's variables, and
  go to the definition of `'#name'`. In `compactGreet()`,
  `'acme_greeting__compact'` is a suggestion: it leads to `acme_greeting` and
  to `acme_theme`'s `acme-greeting--compact.html.twig`.
- A theme hook's templates receive its variables, typed by the render
  arrays that pass them, and the variables Drupal adds to every template.
  In `web/modules/custom/acme/templates/acme-greeting.html.twig`, hover
  `name` to see the `string` `GreetingController::greet()` passes, go to its
  definition in `AcmeHooks::theme()`, or complete after `attributes.` to see
  core's `Attribute` methods.
- Library names navigate between the libraries file that declares them and
  the render arrays, templates, and YAML that attach them. In
  `web/modules/custom/acme/src/Controller/GreetingController.php`, hover
  `'acme/greeting'` in the `#attached` of `greet()` to see its libraries file
  and version, or go to its definition, which lands on its key in
  `acme.libraries.yml`; the `attach_library()` in
  `templates/acme-greeting.html.twig`, the `dependencies` of
  `web/themes/custom/acme_theme/acme_theme.libraries.yml`, and the
  `libraries` of `acme_theme.info.yml` go there too. Complete a library name
  in any of them; core's libraries, such as `core/drupal`, are offered too.
  A name no libraries file declares is reported, unless its module adds
  libraries in code with `hook_library_info_build()` or
  `hook_library_info_alter()`.
- A module's service provider may build a service from a class of its own.
  In `acme_french_site_name()` in `web/modules/custom/acme/acme.module`,
  hover `$languageManager`: core declares `language_manager` as
  `LanguageManager`, and the language module's `LanguageServiceProvider`
  swaps in `ConfigurableLanguageManager`, so it is either class.
- A service that decorates another takes over its ID. In
  `acme_request_age()` in `web/modules/custom/acme/acme.module`, hover
  `$clock`: core declares `datetime.time` as `Time`, and
  `acme.services.yml` decorates it with `RequestClock`, so it is either
  class and `secondsSinceRequest()` is no unknown method.
- Render element names in `#type` navigate to the element class that
  declares them. In `card()` in
  `web/modules/custom/acme/src/Controller/GreetingController.php`, go to the
  definition of `'acme_greeting_card'`, which lands on
  `src/Element/GreetingCard.php`, or hover `'details'` to see core's
  `Details` class. Complete a name inside `'#type' => ''` or
  `$build['more']['#type'] = ''`; core's elements are offered too. A name
  no element class declares is reported.
- Plugin IDs navigate to the plugin class that declares them, for every
  plugin type a plugin manager declares, core's and contributed modules'
  included. In `block()` in
  `web/modules/custom/acme/src/Controller/GreetingController.php`, go to the
  definition of `'acme_greeting_block'`, which lands on
  `src/Plugin/Block/GreetingBlock.php`, or complete a block ID inside
  `createInstance('')`. The block it creates is a `GreetingBlock`, so
  `greeting()` goes to its method. In
  `web/modules/custom/acme/src/Entity/Note.php`, the field type
  `BaseFieldDefinition::create()` takes, the widget the form display
  options name, and the constraint `addConstraint()` takes navigate to
  core's plugin classes and complete. Misspell `'string'` as `'strin'` and
  the field type is reported, since no plugin declares it. Go to the
  implementations of `BlockManagerInterface` to list every block.
- Annotations on a plugin class find their class the way plugin discovery
  does, without a `use` import. In
  `web/modules/custom/acme/src/Plugin/Block/FarewellBlock.php`, go to the
  definition of `@Block`, which lands on core's block annotation class, or
  of `@Translation`, which lands on core's `Translation` annotation.

## What the Symfony support already reads

Drupal declares some things in shapes PHPantom already reads for any
project. What works today:

- **Class names and `Class::method` strings in YAML.** In
  `web/modules/custom/acme/acme.routing.yml`, the `_controller` string goes
  to `GreetingController` and to its `greet()` method, and in
  `web/modules/custom/acme/acme.services.yml`, `class:` goes to `Greeter`.
  Each counts among the references of the class or method it names.
- **Methods under `calls:`.** In
  `web/modules/custom/acme/acme.services.yml`, `setStringTranslation` goes
  to the method `Greeter` takes from core's `StringTranslationTrait`, and a
  method name there completes from the public methods of the service's
  class.

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
