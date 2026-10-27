# Yii Demo Project for PHPantom LSP

A standalone editor playground for PHPantom's Yii 3 support. It is the
application `yiisoft/app` generates, with the tooling trimmed off and a small
blog added: a route group, a controller, views rendered through an alias, and
a DI definition, each declared the way a real Yii 3 project declares them.

## What to try

- Open `config/common/routes.php`. The `blog/` group's routes name their
  handlers as `[PostController::class, 'index']` and
  `[PostController::class, 'view']`. Go to the definition of `'view'`, find
  its references, or rename it, and the method in
  `src/Web/Blog/PostController.php` follows.
- Open `config/common/di/blog.php`. Go to the definition of
  `InMemoryPostRepository`, the class the container builds for
  `PostRepositoryInterface`. Its constructor's `$posts`, its `setLogger()`
  method, and its `$pageSize` property are the names the definition's keys
  spell.
- In `PostController::view()`, hover `$post` or complete after `$post->`. The
  repository's interface declares what `findBySlug()` returns.
- Open `resources/views/blog/index.php`. Hover `$post` inside the loop, or
  complete after `$post->`. The template declares the variables the
  controller passes it with `@var`.

## Getting started

1. Run `composer install` in this directory to install Yii.
2. Open this directory as a project or workspace folder in your editor.

`php assertions.php` builds the configuration and DI container the way Yii's
web runner does and checks every name the demo declares. To see the
application itself, run `php -S localhost:8080 -t public` and open
`http://localhost:8080/blog`.
