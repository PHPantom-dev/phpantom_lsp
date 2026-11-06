# Symfony Demo Project for PHPantom LSP

A standalone editor playground for PHPantom's Symfony and Doctrine support. It
uses attributes, YAML, XML, PHP configurators, Twig, and translation
catalogues together, the way a real Symfony project does.

## What to try

- Open `config/services.yaml`, `config/routes.yaml`,
  `config/doctrine/User.orm.xml`, or `config/validator/User.yaml` and use
  go-to-definition on a PHP class name, or on the `Class::method` in a route's
  `controller`.
- Use find references on a class or method mentioned there, such as
  `DemoController::legacy()`. The YAML and XML occurrences are included.
- Rename `legacy` in `config/routes.yaml`, or rename `UserRepository` from
  `config/doctrine/User.orm.xml`. The PHP declaration and every configuration
  file that names it are renamed together.
- Service IDs and container parameters navigate between the YAML and PHP
  configuration and the code that reads them. In
  `src/Controller/DemoController.php`, go to the definition of
  `app.welcome_mailer` or `app.sender_name`, find their references, or
  complete a name inside `$container->has('')` or
  `#[Autowire(param: '')]`. The names the installed bundles declare complete
  and navigate too: in `config/services.php`, go to the definition of
  `cache.app` to reach FrameworkBundle's own configuration. A container
  builder declares them as well: `app.default_mailer` in `DemoController`
  leads to `src/DependencyInjection/AcmeDemoExtension.php`. A `%name%`
  placeholder in a configurator's string, such as `app.greeting`'s, reads
  the parameter it names.
- Route names navigate between their `#[Route]` attribute or
  `config/routes.yaml` entry and the controllers and Twig templates that
  link to them. In `templates/demo/index.html.twig`, go to the definition of
  `app_demo` or `app_demo_legacy`, find references on `app_demo` in
  `DemoController`, or complete a name inside `generateUrl('')` or
  `path('')`. Names are followed through the imports that load them: in
  the same template, `admin_dashboard` is `src/Admin/DashboardController.php`'s
  `dashboard` under the import's `name_prefix`, and `_preview_error` is
  declared by FrameworkBundle's own routing file. A route name nothing
  declares is reported, in a template too.
- Translation keys navigate between the YAML and XLIFF catalogues in
  `translations/` and the PHP and Twig that translate them, within the
  domain each call names. In `DemoController`, go to the definition of
  `demo.title` or `user.invalid_email`, or complete a key inside
  `trans('')`. A key spelled like one (`demo.titel`) that nothing declares
  is reported, in a domain the project's own catalogues hold and no
  installed package ships a catalogue for.
- Template names navigate between the controllers and templates that render,
  extend, or include them and the files in `templates/`. In
  `DemoController`, go to the definition of `demo/index.html.twig`, or
  complete a name inside `render('')` or `{% include '' %}`. A template that
  does not exist is reported, in PHP and Twig alike, with quick fixes that
  change it to a close name or create it.
- Blocks navigate between the templates that override them. In
  `templates/demo/index.html.twig`, go to the definition of `title` or of
  `parent()` to reach `base.html.twig`'s block, and in `base.html.twig` go to
  the definition of `body` to reach the template that overrides it.
- Twig filters and functions lead to the PHP behind them. In
  `templates/demo/index.html.twig`, go to the definition of `initials` to
  reach `src/Twig/AppExtension.php`, of `badge` to reach the
  `#[AsTwigFunction]` in `src/Twig/Badges.php`, or of `upper` to reach
  Twig's own extension. Hover one for its signature, complete a filter after
  `|`, and misspell one to see it reported.
- Template variables lead to what passes them. In
  `templates/demo/index.html.twig`, go to the definition of `sender` to reach
  the key `DemoController` renders the template with, or of `support_url` to
  reach the global `config/packages/twig.yaml` declares. In
  `templates/demo/_card.html.twig`, `sender` comes through the template that
  includes it. Hover a variable to see where it comes from and what it
  holds, and complete one where an expression starts. Go to the definition
  of `displayName` in `user.displayName` to reach `User::getDisplayName()`,
  and complete after `user.` to see what else the class has.
- Form fields and validation mappings name the properties of the class they
  map. In `src/Form/RegistrationType.php` or `config/validator/User.yaml`, go
  to the definition of `email`, find the references of `$displayName` in
  `src/Entity/User.php`, or complete a field inside `->add('')`. A constraint
  such as `NotBlank` goes to its class.
- Form options complete from the form type. In
  `src/Form/RegistrationType.php`, complete a key in the options array of
  the `email` field, go to the definition of `'help'`, or misspell it to see
  it reported. A constraint's options are its constructor's parameters, so a
  misspelled `max` in `Length(max: 180)` is reported too.
- Constraint options complete and are checked in validation mappings too. In
  `config/validator/User.yaml`, complete an option of `Length` on a new line
  under `max`, or misspell `min` to see it reported.
- Serializer groups navigate between the `#[Groups]` that put a member in
  them and the contexts that serialize with them. In `DemoController`, go to
  the definition of `user:list` in `profile()` to reach
  `User::getDisplayName()`, find the references of `user:read` in
  `src/Entity/User.php`, or complete a group in the `groups` list.
- Bundle configuration keys navigate between the schema in
  `src/DependencyInjection/Configuration.php` and
  `config/packages/acme_demo.yaml`. Go to the definition of `dsn`, find the
  references of `'api_key'`, or complete a key on a new line under
  `mailer:`.
- Event names navigate from the dispatcher to every listener that subscribes
  to them. In `DemoController`, go to the definition of `app.order.placed`
  to reach the `#[AsEventListener]` in
  `src/EventListener/OrderPlacedListener.php` and the tag in
  `config/services.yaml`, or complete a name inside `dispatch($event, '')`.
- Messenger buses navigate between `config/packages/messenger.yaml` and the
  handlers that name them. In
  `src/MessageHandler/SendWelcomeEmailHandler.php`, go to the definition of
  `command.bus`. The CodeLens above its `__invoke()` opens the message it
  handles, and the one above `src/Message/SendWelcomeEmail.php` opens the
  handler.
- Services, parameters, and query results are typed by what the
  configuration declares. In `DemoController`, hover `$mailer`, fetched by
  the service ID `app.mailer`, or `$supportEmail`, read with
  `getParameter()`. In `src/Query/QueryBus.php`, hover `$count`, which
  `HandleTrait::handle()` gets back from the message's handler.
- Open `src/Entity/User.php` and `src/Repository/UserRepository.php`. The
  CodeLens above each class links the entity and its repository.
- In `DemoController`, hover `$users`. `getRepository(User::class)` returns
  the `UserRepository` the Doctrine mapping names, so its own
  `withEmail()` completes.
- Below it, complete after `$users->findOneBy`. The repository answers a
  `findBy<Field>()`, `findOneBy<Field>()`, and `countBy<Field>()` for each
  field `config/doctrine/User.orm.xml` maps, and the keys of `findBy()`
  complete those fields and jump to their properties.
- In `UserRepository::named()`, complete after `u.` in the query builder
  or the DQL string. The fields of the entity the variable stands for
  complete and jump to their properties, the entity class in `FROM` jumps
  to the class, and a field the mapping does not map is reported.
- In `UserRepository::named()`, hover `$byBuilder` or `$byQuery`: a query
  is typed by what its DQL selects, so both are a `list<User>`. In
  `displayNamesByEmail()`, hover `$rows`, a shape of the selected fields
  keyed by the `INDEX BY` field.

## Getting started

1. Run `composer install` in this directory to install Symfony and Doctrine.
2. Open this directory as a project or workspace folder in your editor.

The files are a language-server playground, not a bootable Symfony
application, so no kernel or database is required.
