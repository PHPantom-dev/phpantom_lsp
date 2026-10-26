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
  `#[Autowire(param: '')]`.
- Route names navigate between their `#[Route]` attribute or
  `config/routes.yaml` entry and the controllers and Twig templates that
  link to them. In `templates/demo/index.html.twig`, go to the definition of
  `app_demo` or `app_demo_legacy`, find references on `app_demo` in
  `DemoController`, or complete a name inside `generateUrl('')` or
  `path('')`.
- Translation keys navigate between the YAML and XLIFF catalogues in
  `translations/` and the PHP and Twig that translate them, within the
  domain each call names. In `DemoController`, go to the definition of
  `demo.title` or `user.invalid_email`, or complete a key inside
  `trans('')`.
- Template names navigate between the controllers and templates that render,
  extend, or include them and the files in `templates/`. In
  `DemoController`, go to the definition of `demo/index.html.twig`, or
  complete a name inside `render('')` or `{% include '' %}`. A template that
  does not exist is reported, with a quick fix that creates it.
- Form fields and validation mappings name the properties of the class they
  map. In `src/Form/RegistrationType.php` or `config/validator/User.yaml`, go
  to the definition of `email`, find the references of `$displayName` in
  `src/Entity/User.php`, or complete a field inside `->add('')`. A constraint
  such as `NotBlank` goes to its class.
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

## Getting started

1. Run `composer install` in this directory to install Symfony and Doctrine.
2. Open this directory as a project or workspace folder in your editor.

The files are a language-server playground, not a bootable Symfony
application, so no kernel or database is required.
