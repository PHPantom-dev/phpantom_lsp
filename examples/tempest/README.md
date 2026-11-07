# Tempest View playground

Run `composer install`, then open this directory in an editor using PHPantom.
The `.view.php` files use the editor's PHP language-server connection.

- In `app/BookController.php`, complete or follow the view path.
- In `app/books.view.php`, complete directives and component tags, follow
  `<x-card>` to its file, and complete or navigate `$book`'s members.
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
components with Tempest’s own discovery runner.

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
