# Tempest View playground

Run `composer install`, then open this directory in an editor using PHPantom.
The `.view.php` files use the editor's PHP language-server connection.

- In `app/BookController.php`, complete or follow the view path.
- In `app/books.view.php`, complete directives and component tags, follow
  `<x-card>` to its file, and complete or navigate `$book`'s members.
- In `app/branches.view.php`, inspect the loop variable across sibling branches.
- In `app/components/x-card.view.php`, hover `$title` for its declared type.

Template PHP uses imports, assignments and `@var` annotations, including an
annotation for `$this` when a view object supplies its members. Annotate data
passed from controllers or other components; automatic inference from those
call sites and isolated slot scopes are tracked in the [backlog](../../docs/todo/lsp-features.md#f73-tempest-view-context-inference). Components use Composer discovery roots; explicit registrations and extra discovery locations are [follow-up work](../../docs/todo/lsp-features.md#f74-configured-tempest-view-discovery).

Verify the playground with:

```sh
find app -name '*.php' -print0 | xargs -0 -n1 php -l
php assertions.php
phpantom_lsp analyze --project-root . --no-colour
```

The assertions render the templates with Tempest's own renderer, checking
the component, loop variable and empty-loop behavior demonstrated above.

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
