# Symfony Demo Project for PHPantom LSP

A standalone editor playground for PHPantom's Symfony and Doctrine
intelligence. It intentionally uses attributes, YAML, XML, PHP configurators,
Twig, and translation catalogues together.

## What to try

- Open `src/Controller/DemoController.php` and use completion,
  go-to-definition, find references, rename, and diagnostics on route names,
  route parameters, service IDs, parameters, templates, translations, and
  event names.
- Open `src/Entity/User.php` to see code lenses for Doctrine mappings, form
  fields, and validation properties.
- Open `src/Message/SendWelcomeEmail.php` or its handler to navigate the
  Messenger relationship in either direction.
- Open `config/services.php` to navigate between a PHP service declaration and
  its class. YAML and XML references work the same way.
- Open `config/packages/acme_demo.yaml` to complete and navigate keys from the
  local `TreeBuilder` schema.

## Getting started

1. Optionally run `composer install` here to install the real Symfony classes.
2. Open this directory as a project or workspace folder in your editor.
3. Trigger completion or go-to-definition inside the example strings.

The files are a language-server playground, not a bootable Symfony
application, so no kernel or database is required.
