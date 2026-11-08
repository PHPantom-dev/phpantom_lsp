# Tempest playground

Run `composer install`, then open this directory in an editor using PHPantom.
The `.view.php` files use the editor's PHP language-server connection.

- In `app/BookController.php`, complete or follow the view path.
- In `app/DatabaseQueries.php`, complete and follow model property names in
  query builders and model helpers.
- In `app/RouteLinks.php`, complete and follow route URIs and their arguments.
  In `BookController.php`, follow route placeholders to parameters and rename them
  together with their URI arguments. Routes without a leading slash complete
  as absolute paths. Custom route attributes and controller
  prefixes work too.
- In `app/books.view.php`, complete directives and component tags, follow
  `<x-card>` to its file, and complete or navigate `$book`'s members.
  Follow `:title` and `:show-title` to their first variable reads in the
  component, or hover their names to inspect the component's types.
- In `app/dynamic.view.php`, follow a dynamic component name, complete its
  attributes, and inspect the inferred data in the selected component and slot.
- In `app/branches.view.php`, inspect the loop variable across sibling branches.
- In `app/components/x-card.view.php`, hover `$title` for its declared type.
- In `app/book.view.php`, inspect the custom View through `$this` and the
  inferred types in component arguments and slots.

Template PHP infers data from callers and custom View objects. Component
slots use the receiving component's scope, and explicit `@var` annotations
can refine inferred types. Components use Composer and configured discovery
roots, including explicit registrations. The files in `app/*.config.php`
configure components outside the autoload root; `ConfiguredViews.php` passes
a constant path through a View helper. In `resources/configured.view.php`,
complete or follow the configured component tags and inspect the inferred book data.

Verify the playground with:

```sh
find app resources ui -name '*.php' -print0 | xargs -0 -n1 php -l
php assertions.php
phpantom_lsp analyze --project-root . --no-colour
```

The assertions render the templates with Tempest's own renderer, checking
the data and scope behavior demonstrated above, and discover configured
components with Tempest's own discovery runner. They also discover routes and
generate the demo links with Tempest's router and run the model queries
against an in-memory SQLite database.

Performance checks use this playground's declarations with a generated
catalogue of 400 views. Completion and navigation results are checked before
timing or sampling memory:

```sh
cargo bench --bench tempest
python3 benches/memory_usage.py --scenario tempest_view --binary target/release/phpantom_lsp
```

Run those commands from the repository root after `cargo build --release`.
The memory check installs the playground's locked dependencies in a temporary
directory and requires PHP 8.5. CI runs both benchmarks automatically.
