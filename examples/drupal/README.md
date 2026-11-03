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
