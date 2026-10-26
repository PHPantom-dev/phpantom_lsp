# PHPantom — WordPress

Known gaps and missing features in PHPantom's WordPress support. Today
WordPress gets only the generic engine: there is no WordPress-specific
code, and core's functions and classes resolve only when the project
indexes WordPress itself or a stub package for it.

Most WordPress typing lives in `php-stubs/wordpress-stubs`, so the
external stubs are the foundation for everything here. Installed with
Composer, the package is a stub source with no configuration, as is
`php-stubs/woocommerce-stubs`; a project without Composer can list
either under `[stubs] paths`. `global $wpdb;` being untyped is
a gap in the generic engine, tracked as
[T57](type-inference.md#t57-a-global-variable-is-untyped-unless-a-var-sits-above-it).
PHPStan's WordPress extension
(`szepeviktor/phpstan-wordpress`) is the checklist of what a WordPress
developer expects from types; PhpStorm's built-in WordPress support and
PHP Tools' hook navigation are what they expect from an editor.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## WP1. WordPress demo project and a real-world baseline

**Impact: High · Complexity: Low-Medium**

Every other WordPress item is measured against this one. Without a demo
project and a baseline from real code, WordPress support would be built
untested and could regress silently.

- **`examples/wordpress/`**: a small plugin and a theme, in the same
  shape as `examples/laravel/` and `examples/symfony/`, with "Try:"
  comments at each site and a README listing what to try. It installs
  `php-stubs/wordpress-stubs` through Composer, which is how most typed
  WordPress projects get core's declarations.
- **Runtime assertions**: an `assertions.php` that checks the demo's
  claims against real WordPress, as `examples/laravel/assertions.php`
  does for Eloquent. Decide whether that boots WordPress (core from
  Composer plus the SQLite database drop-in, so no server is needed) or
  only checks what can be read without booting.
- **A real-world baseline**: run `analyze` on an actively maintained
  open-source plugin and theme, bucket the diagnostics by cause, and
  file what WordPress-specific gaps they show. That ranks the rest of
  this document by what actually breaks.

---

## WP2. Audit what the WordPress stubs declare against the engine

**Impact: High · Complexity: Low**

`php-stubs/wordpress-stubs` carries more than signatures. Recent
versions declare argument-dependent return types, `@phpstan-assert`
narrowing and never-returning functions for a large part of core,
absorbed from PHPStan's WordPress extension. Examples to check:

```php
$post = get_post($id, ARRAY_A);   // array shape, not WP_Post
if (is_wp_error($result)) { … }   // narrows $result to WP_Error
wp_die();                         // nothing after it is reachable
```

Go through what the stubs declare, write an assertion fixture for each
construct, and file every one the engine reads wrongly as a bug in
`bugs.md` or `type-inference.md`. Fixing those fixes them for every
library that uses the same annotations, not just WordPress. What the
stubs do not declare at all goes to WP7.

---

## WP3. WordPress core for a plugin or theme opened on its own

**Impact: Medium-High · Complexity: Medium**

A WordPress developer usually opens one plugin or theme
(`wp-content/plugins/acme-shop/`), not the whole install. Core then lies
outside the workspace, so `WP_Query`, `add_action()`, and `$wpdb` do not
resolve unless the plugin happens to require `wordpress-stubs`.

When the workspace sits inside a WordPress install (a `wp-includes/` and
`wp-load.php` some levels up, past `wp-content/plugins/` or
`wp-content/themes/`), index core as library code, the role
`[indexing] include` gives a path outside the workspace. Sibling
plugins the code calls into (WooCommerce, ACF) are the same question one
level over; decide whether to index the whole `wp-content/plugins/`
tree or only what the workspace references.

This follows the rule in `external-stubs.md`: Drupal's web root and
sibling editor folders are detected, never typed by the user, and every
new source of PHP maps onto an existing role.

**Where to look:** `src/classmap_scanner/discovery.rs`
(`scan_drupal_directories` is the closest precedent),
`src/config/indexing.rs`.

---

## WP4. Hook names

**Impact: High · Complexity: Medium**

```php
add_action('acme_order_placed', [$this, 'sendReceipt']);

do_action('acme_order_placed', $order);
```

The hook name is the string that ties a listener to the place that fires
it, and nothing navigates between the two today. Treat it as a string
key, the way Laravel's route, config, and translation names are:

- **Declarations**: `do_action()`, `do_action_ref_array()`,
  `do_action_deprecated()`, `apply_filters()`,
  `apply_filters_ref_array()`, `apply_filters_deprecated()`.
- **Uses**: `add_action()`, `add_filter()`, `remove_action()`,
  `remove_filter()`, `has_action()`, `has_filter()`, `did_action()`,
  `doing_action()`.
- **Features**: completion of hook names in a use, go-to-definition from
  a use to every place that fires it, find references, rename, and
  document highlight.
- **Dynamic names**: core fires many hooks with an interpolated name
  (`"save_post_{$post->post_type}"`). Match a use against the literal
  parts, and offer the pattern in completion.

Reporting a hook nothing fires is not safe as a default: a plugin
listens to hooks that other plugins or a future core version fire. If it
is done at all, it belongs behind an opt-in.

PHP Tools added hook navigation in 2026, and PhpStorm has it built in,
so this is the feature a WordPress developer will compare first.

**Where to look:** the string-key machinery Laravel and Symfony names
use (`src/virtual_members/laravel/string_keys.rs`,
`src/symbol_map/extraction/symfony.rs`).

---

## WP5. Hook callback parameter and return types

**Impact: Medium-High · Complexity: Medium-High** (depends on WP4)

```php
/**
 * Filters the post content.
 *
 * @param string $content Content of the current post.
 */
$content = apply_filters('the_content', $content);

add_filter('the_content', function ($content) {
    return $content . '…';   // $content is untyped today
});
```

Core documents every hook with a docblock above the call that fires it.
Its `@param` tags type the callback's parameters, and a filter's first
`@param` is also what the callback must return and what
`apply_filters()` returns. With WP4 linking a callback to its hook, the
closure parameter inference that already feeds `array_map()` callbacks
can feed hook callbacks from the same docblock.

PHPStan's WordPress extension also checks a callback against the hook:
too few accepted arguments for the parameters it declares (the fourth
argument of `add_action()`), and a filter callback that returns nothing.
Those are good diagnostics once the types are there.

**Where to look:**
`src/type_engine/variable/forward_walk/callable_inference.rs`, and PHPStan's
WordPress extension (`HookCallbackRule`, `ApplyFiltersDynamicFunctionReturnTypeExtension`).

---

## WP6. Template parts

**Impact: Medium · Complexity: Medium**

```php
get_template_part('template-parts/content', get_post_type());
locate_template(['parts/hero.php']);
```

Theme code names template files by a path without the extension, looked
up in the child theme and then the parent theme. Navigate from the
string to the file, complete it from the theme's PHP files, and report a
literal name no file matches. The `$args` array passed to
`get_template_part()` arrives in the part as `$args`, the same shape of
problem as Blade and Twig template variables.

**Where to look:** Laravel's view names (`src/virtual_members/laravel/view_names.rs`)
and the Twig template-name support.

---

## WP7. Return types the WordPress stubs do not declare

**Impact: Medium · Complexity: Medium** (depends on WP2)

Whatever WP2 finds that PHPStan's WordPress extension types and the
stubs do not, typically a return type an argument decides:
`get_posts(['fields' => 'ids'])` returning `int[]`,
`get_post_meta($id, $key, true)` returning a single value rather than an
array, `wp_parse_url($url, PHP_URL_HOST)`. Roots' Sage theme shows more:
`wp_list_comments([..., 'echo' => false])` is declared `void|string`
whatever `echo` says, and `get_post_time('c')` keeps the `int` only the
`'U'` and `'G'` formats return. Write each as a conditional
`@return` in an override stub
([E7](external-stubs.md#e7-stub-based-framework-patches)) rather than in
Rust, and offer the ones that hold upstream to `php-stubs/wordpress-stubs`.

---

## WP8. Sage view composers

**Impact: Medium · Complexity: Medium**

Roots' Sage theme, through its Acorn framework, hands data to Blade views
with composer classes under `app/View/Composers` that extend
`Roots\Acorn\View\Composer`. Acorn registers every one itself, for the
views its `protected static $views` lists (`'*'` for all of them), or a
view named after the class when it lists none. A view gets the
composer's public methods and properties as variables, each method
called, or the keys of `with()` and `override()` when the class returns
any from them. Nothing registers them through `View::composer()`, so
in a stock Sage theme `$title`, `$pagination` and `$siteName` are
reported as undefined in every partial that reads them.

Feed these into the provider-registered variables the Blade declaration
chain already reads for `View::composer()`, typed from each method's
return type.

**Where to look:** `src/blade/shared_vars.rs` and
`src/virtual_members/laravel/view_data.rs`; Acorn's
`Roots\Acorn\View\Composer` and its `Composers\Concerns\Extractable`
for exactly which members count.
