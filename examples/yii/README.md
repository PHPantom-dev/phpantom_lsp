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
- Open `resources/views/blog/index.php`. Go to the definition of
  `'blog/post'` in `$urlGenerator->generate()`, which lands on the route that
  declares it under its group's `blog/` prefix. Complete route names there,
  and the `'slug'` key, which is the placeholder the route's path declares.
  In `PostController::view()`, `#[RouteArgument('slug')]` goes to that
  `{slug}` and completes from it, and a name the path does not declare is
  reported.
- Open `config/common/di/blog.php`. Go to the definition of
  `InMemoryPostRepository`, the class the container builds for
  `PostRepositoryInterface`. The definition's keys name what they configure:
  `'posts'` goes to the constructor's `$posts`, `'setLogger()'` to the method,
  and `'$pageSize'` to the property. Rename `setLogger()` in
  `src/Blog/InMemoryPostRepository.php` and the key follows. Clear a key to
  complete it, or misspell one to see it reported.
- Open `src/Blog/PostRepositoryInterface.php`. The lens above the interface,
  and go-to-implementation on its name, lead to the definition that binds it.
  Go-to-implementation on `LoggerInterface` in `Reference::to()` does the same
  for the logger.
- In `PostController::view()`, hover `$post` or complete after `$post->`. The
  repository's interface declares what `findBySlug()` returns.
- Open `resources/views/blog/index.php`. Hover `$post` inside the loop, or
  complete after `$post->`. The template declares the variables the
  controller passes it with `@var`.
- In `PostController::index()`, go to the definition of `'index'`. The
  controller's `withViewPath('@views/blog')` and the `@views` alias in
  `config/common/aliases.php` make it `resources/views/blog/index.php`. Clear
  the name to complete it from the views there, and misspell it to see it
  reported. `src/Web/HomePage/Action.php` names its template as
  `__DIR__ . '/template'` instead, and `index.php` renders `'_card'` from
  beside itself. Open `resources/views/blog/_card.php` for the lens listing
  the calls that render it.
- In `PostController::view()`, go to the definition of the
  `withLayout()` argument, which opens `src/Web/Shared/Layout/Main/layout.php`.
  It completes like a view name, and a layout with no file is reported, as is
  a `withViewPath()` directory that does not exist.
- Open `src/Web/Shared/Layout/Main/MainAsset.php`. Go to the definition of
  `'@assetsSource'` in `$sourcePath`, which lands on its declaration in
  `config/common/aliases.php`, or hover it for the path it stands for. The
  rest of the string, `/main`, links to the `assets/main` directory, and an
  `@` typed in any string completes the declared aliases.
- Open `config/common/di/application.php`. Go to the definition of `'name'` in
  `$params['application']['name']`, which lands on its entry in
  `config/common/application.php`, or hover it for the type of its value.
  Clear a key to complete the ones the params files declare beside it, and
  misspell one to see it reported.
- Open `resources/views/blog/post.php`. Go to the definition of
  `'back-link'` in `->class()`, which lands on its selector in
  `assets/main/site.css`, or hover it for the stylesheets that declare it.
  Clear the string to complete the classes the project's stylesheets
  declare, a name at a time.
- Open `vendor/yiisoft/router/composer.json` once the dependencies are
  installed. Its `config-plugin` lists the files the package adds to each
  config group: go to the definition of `"di.php"` for `config/di.php`, clear
  a group name to complete the groups `config/configuration.php` and the
  installed packages use, and misspell a file name to see it reported. A
  `"$group"` entry goes to the group it names.

## Getting started

1. Run `composer install` in this directory to install Yii.
2. Open this directory as a project or workspace folder in your editor.

`php assertions.php` builds the configuration and DI container the way Yii's
web runner does and checks every name the demo declares. To see the
application itself, run `php -S localhost:8080 -t public` and open
`http://localhost:8080/blog`.
