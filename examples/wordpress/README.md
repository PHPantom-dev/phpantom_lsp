# WordPress Demo Project for PHPantom LSP

A standalone editor playground for PHPantom's WordPress support: a small
plugin (`wp-content/plugins/acme-shop/`) and a classic theme
(`wp-content/themes/acme-theme/`). WordPress's functions and classes come
from `php-stubs/wordpress-stubs`, installed with Composer, which is how
most typed WordPress projects get core's declarations.

## What to try

- Go to the definition of a WordPress function or class, such as
  `register_post_type()` in `src/ProductPostType.php` or `WP_Post` in
  `src/Product.php`. It opens the declaration in the stubs.
- Return types follow the arguments. In `src/ProductRepository.php`, hover
  `$post` (`get_post()` gives a `WP_Post` or `null`, and an array with
  `ARRAY_A`), the two `get_posts()` calls (posts, or IDs with
  `'fields' => 'ids'`), and `$id` after `wp_insert_post($data, true)`. In the
  theme's `functions.php`, hover `$price` from `get_post_meta(…, true)`,
  `$terms` from `get_the_terms()`, and `$dir` from `trailingslashit()`.
- `wp_parse_args()` and `shortcode_atts()` return the keys of their defaults.
  Hover `$args` in `ProductRepository::latest()` or `$atts` in
  `src/ProductShortcode.php`, or complete a key inside `$args['']`.
- `is_wp_error()` narrows. In `ProductRepository::create()`, `$id` is an
  `int` once the error has been returned.
- A `@global` tag types the global a function imports. Complete after
  `$wpdb->` in `ProductRepository::countInStock()`, or after `$wp_query->`
  in the theme's `acme_theme_last_page()`.

## Getting started

1. Run `composer install` in this directory to install the stubs, and the
   WordPress core and SQLite driver `assertions.php` uses.
2. Open this directory as a project or workspace folder in your editor.

`php assertions.php` installs WordPress into a throwaway SQLite database,
loads the plugin and theme, and checks the claims above against it. It
needs the `pdo_sqlite` extension, but no web server or MySQL.
