# Yii Demo Project for PHPantom LSP

A standalone editor playground for PHPantom's Yii 3 support. It is the
application `yiisoft/app` generates, with the tooling trimmed off and a small
blog added: a route group, a controller, views rendered through an alias, a
DI definition, and a book catalog kept in Active Records, each declared the
way a real Yii 3 project declares them.

## What to try

- Open `config/common/routes.php`. The `blog/` group's routes name their
  handlers as `[PostController::class, 'index']` and
  `[PostController::class, 'view']`. Go to the definition of `'view'`, find
  its references, or rename it, and the method in
  `src/Web/Blog/PostController.php` follows.
- Open `resources/views/blog/index.php`. Go to the definition of
  `'blog/post'` in `$urlGenerator->generate()`, which lands on the route that
  declares it under its group's `blog/` prefix. Complete route names there,
  and the `'slug'` key, which goes to the `{slug}` placeholder the route's
  path declares and shows that path on hover.
  In `PostController::view()`, `#[RouteArgument('slug')]` goes to that
  `{slug}` and completes from it, and a name the path does not declare is
  reported.
- Open `config/common/di/blog.php`. Go to the definition of
  `InMemoryPostRepository`, the class the container builds for
  `PostRepositoryInterface`. The definition's keys name what they configure:
  `'__construct()'` goes to the constructor and `'posts'` to its `$posts`, `'setLogger()'` to the method,
  and `'$pageSize'` to the property. Rename `setLogger()` in
  `src/Blog/InMemoryPostRepository.php` and the key follows. Clear a key to
  complete it, or misspell one to see it reported. Drop the `$` from
  `'$pageSize'` to see the key reported with a fix that puts it back, or set
  it to `'5'` to see the value reported, since the property takes an `int`.
- Open `config/common/events.php`. Go to the definition of `'count'` in
  `[ViewCounter::class, 'count']`, which opens `src/Blog/ViewCounter.php`.
  Clear it to complete `ViewCounter`'s public methods, rename `count()` and
  the string follows, or misspell it to see it reported. Point it at
  `'viewsOf'` to see the listener reported, since its `string $slug` can be
  filled with neither the event nor a service, and drop the `PostViewed`
  type from the closure's `$event` for the same report. Open
  `src/Blog/PostViewed.php` for the lens listing its listeners, which
  go-to-implementation on the class name lists too.
- Open `resources/views/blog/post.php`. Go to the definition of `'author'`
  in `Field::text($comment, 'author')`, which lands on the property in
  `src/Web/Blog/CommentForm.php`. Clear the string to complete the form's
  properties, misspell it to see it reported, or rename the property and
  both fields follow. In `PostController::comment()`, the keys of the map
  `populateFromPost()` is given name the same properties, and navigate,
  complete, and are checked the same way.
- Open `src/Blog/PostRepositoryInterface.php`. The lens above the interface,
  and go-to-implementation on its name, lead to the definition that binds it.
  Go-to-implementation on `LoggerInterface` in `Reference::to()` does the same
  for the logger.
- In `PostController::view()`, hover `$post` or complete after `$post->`. The
  repository's interface declares what `findBySlug()` returns.
- Open `resources/views/blog/index.php`. Hover `$post` inside the loop, or
  complete after `$post->`. The template declares the variables the
  controller passes it with `@var`.
- In `PostController::index()`, go to the definition of `'posts'` in the
  array `render()` passes, which lands on its `@var` in
  `resources/views/blog/index.php`, or hover it for its type. Clear a key to
  complete the parameters the template declares. A value the declaration does
  not accept, a key the template has no use for, and a declared parameter
  the call leaves out are reported. `$urlGenerator` is not one the call has
  to pass: `config/common/params.php` gives it to every view.
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
  `@` typed in any string completes the declared aliases. Search workspace
  symbols for `@assetsSource` to find its declaration, and in an editor that
  shows folded text, fold the string to see the path it resolves to. Go to the
  definition of `'site.css'` in `$css`, which opens `assets/main/site.css`
  under `$sourcePath`. Clear it to complete the files there, and misspell it
  to see it reported.
- Open `config/common/di/application.php`. Go to the definition of `'name'` in
  `$params['application']['name']`, which lands on its entry in
  `config/common/application.php`, or hover it for the type of its value.
  Clear a key to complete the ones the params files declare beside it, and
  misspell one to see it reported.
- Open `src/Web/HomePage/template.php`. Go to the definition of
  `'home.greeting'` in `$translator->translate()`, which lands on its entry in
  both `resources/messages/en/app.php` and `resources/messages/de/app.php`, or
  hover it for the English message. The call names no category, so the ID is
  looked up in the translator's default one, `app`. Clear the ID to complete
  the ones the category declares, and misspell it to see it reported.
  `resources/views/blog/post.php` names the `blog` category with
  `category: 'blog'`, and `PostController` gives its translator that category
  with `withDefaultCategory('blog')`.
- Open `resources/views/blog/post.php`. Go to the definition of
  `'back-link'` in `->class()`, which lands on its selector in
  `assets/main/site.css`, or hover it for the stylesheets that declare it.
  Clear the string to complete the classes the project's stylesheets
  declare, a name at a time.
- Open `src/Catalog/BookCatalog.php`. Hover `$book` in `find()`: the query
  `Book::query()` builds returns a `Book`. Go to the definition of `'author'`
  in `with()`, which lands on `Book::getAuthorQuery()`, and of `'books'` in
  `with('books.author')`, which lands on its arm in `Author::relationQuery()`.
  Each segment of the path is a relation of the record the one before it is
  for. In `describe()`, `'title'` in `get()` goes to `Book::$title`, and
  renaming the property renames the string. Clear a name to complete the
  relations or attributes the record declares, and misspell one to see it
  reported.
  `Review` declares no properties, so in `stars()` its attributes are the
  columns `src/Migration/M251108000000CreateCatalog.php` creates in its
  table: `'rating'` goes to its column there, hovers with its type,
  completes, and is reported when misspelled. The migrator finds the
  migration through the `yiisoft/db-migration` params in
  `config/console/params.php`.
  In `titlesBy()`, a `yiisoft/db` query names its tables and columns as
  strings: `'author'` goes to the `createTable()` that creates it, and
  `'b.title'` to the column of the table the alias `b` stands for, which
  `Table::BOOK . ' b'` names through a constant the migration names it with
  too. Hover a column for its type, clear one to complete the columns of the
  query's tables, and misspell a table or a column to see it reported.
  `titles()` builds its query over several statements, and the columns the
  later ones name are read against the table the first one names: `'b.title'`
  and `'b.author_id'` go to their columns, complete, and are reported when
  misspelled.
- In `BookCatalog::shelvedTitle()`, hover `$title`: `getValueByPath()`
  walks `'book.title'` through the entry and into its `Book`, so it is a
  `string`. Go to the definition of `'title'`, which lands on `Book::$title`.
  Clear the path to complete the entry's keys, and type a dot after `book`
  to complete the properties of `Book`.
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
