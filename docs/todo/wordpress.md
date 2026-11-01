# PHPantom — WordPress

Known gaps and missing features in PHPantom's WordPress support. Today
WordPress gets the generic engine, plus the output constants (`OBJECT`,
`ARRAY_A`, …) its stubs leave out and the return types an argument
decides that they declare too wide (`src/wordpress/`). Core's functions
and classes resolve only when the project indexes WordPress itself or a
stub package for it. The stubs' own type tests run in
`tests/wordpress_stubs/`, and what the engine reads wrongly in them is
filed under "Conditional return types" in [bugs.md](bugs.md).

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

## WP9. The argument types the WordPress stubs declare

**Impact: Medium · Complexity: Low-Medium**

```php
add_menu_page('Acme', 'Acme', 'manage_options', 'acme', 'notACallable');
// Parameter #5 $callback of function add_menu_page expects ''|(callable(): mixed), 'notACallable' given.
```

The stubs narrow many parameters with `@phpstan-param`: shapes for
`$args` arrays, callables with their signatures, literal unions for
modes. Their own rule tests (`tests/data/param/` and
`ParameterTypeTest.php` of `php-stubs/wordpress-stubs`, 31 files) list
the calls each declaration accepts and the errors it must report. The
stubs' type tests are ported in `tests/wordpress_stubs/`; port these
beside them as diagnostics fixtures (as `tests/fixtures/phpstan_symfony/`
does for the Symfony extension's rule tests), extending the slice of the
stubs `scripts/extract-wordpress-stubs.php` copies, and file every call
the argument-type check judges differently.

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
PhpStorm, pointed at a WordPress install, completes core's and the
project's own hook names in `add_action('…')`, navigates from a use to
where the hook fires, and marks both ends in the gutter. Its completion
only opens on an explicit Ctrl+Space; ours opens on the quote, since `'`
and `"` are already completion triggers.

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

## WP13. Return types no conditional `@return` can express

**Impact: Medium · Complexity: Medium**

```php
$args = wp_parse_args($args, ['before' => '', 'echo' => true]);
$args['echo'];   // mixed, want bool
```

`src/wordpress/overrides.php` gives the WordPress stubs the conditional
return types an argument decides. What is left of PHPStan's WordPress
extension needs the engine, since no PHPDoc type can say it. Its
fixtures are ported in `tests/wordpress_stubs/data/phpstan-wordpress/`,
each gap marked `// SKIP`:

- **Merged shapes.** `wp_parse_args($args, $defaults)` of an array is
  `array_merge($defaults, $args)`, which the array function rules
  (`src/type_engine/variable/array_func_rules.rs`) already type.
  `shortcode_atts($pairs, $atts)` keeps the keys of `$pairs`, each value
  its default's type or `string`. 23 assertions.
- **`esc_sql()` keeps the shape** of the array it escapes, each scalar
  value a `string` and anything else `''`. 11 assertions.
- **`wp_slash()` keeps the shape** of the array it slashes, each string
  value slashed. The stubs' conditional gives `array<key-of<T>, …>`, which
  loses the keys each value belongs to. 6 assertions.
- **Calls on literal arguments.** `wp_slash('foo')`,
  `trailingslashit('foo')`, `normalize_whitespace(' 0 ')`,
  `stripslashes_from_strings_only('foo')` and
  `wp_parse_url('http://example.com', PHP_URL_HOST)` give the literal
  WordPress would return, the way `scalar_fold.rs` folds PHP's own string
  functions. 33 assertions.

The overrides that hold for every WordPress version the stubs cover are
worth offering to `php-stubs/wordpress-stubs` as well.

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

---

## WP10. Hook docblocks that do not match the call

**Impact: Low-Medium · Complexity: Medium** (depends on WP5)

```php
/**
 * Filters the order total.
 *
 * @param float $total The total.
 */
$total = apply_filters('acme_order_total', $total, $order);   // 2 arguments, 1 @param
```

WP5 reads the docblock above `do_action()` and `apply_filters()` as the
hook's signature, so a docblock that disagrees with the call types every
callback wrongly. PHPStan's WordPress extension (`HookDocsRule`) reports
three mismatches: a `@param` count that differs from the arguments after
the hook name, a `@param` named `$this` or written with invalid syntax,
and a `@param` type the argument passed does not satisfy. Report the
same, on the docblock, for the plugin and theme authors who document
their own hooks. Its rule test (`tests/data/hook-docs.php`) lists the
cases.

---

## WP11. `assertWPError()` and `assertNotWPError()` narrow

**Impact: Low · Complexity: Low**

```php
$result = wp_insert_post($data, true);   // int|WP_Error
$this->assertNotWPError($result);
$result + 1;                             // still int|WP_Error
```

WordPress's PHPUnit base class (`WP_UnitTestCase_Base`) asserts a value
is or is not a `WP_Error`, and the test stubs do not say so. PHPStan's
WordPress extension narrows through both
(`AssertWpErrorTypeSpecifyingExtension`, tested in
`tests/data/assert_wp_error.php` and `assert_not_wp_error.php`). Declare
them with `@phpstan-assert WP_Error $actual` and
`@phpstan-assert !WP_Error $actual` in an override stub
([E7](external-stubs.md#e7-stub-based-framework-patches)) rather than in
Rust.

---

## WP12. Constants `wp-config.php` defines keep the value of one install

**Impact: Low · Complexity: Low-Medium**

```php
// wp-config.php
define('WP_CONTENT_DIR', '/var/www/wp-content');

// a plugin
if (WP_CONTENT_DIR === '/srv/site/wp-content') { … }   // read as `false`
```

A constant from a `define()` in `wp-config.php` is typed as the literal
it holds on this machine, so a comparison against another install's value
is decided as `false`. The
value differs between installs, so its type is the general one (`string`,
`bool`). PHPStan's WordPress extension lists these
(`dynamicConstantNames` in `extension.neon`: `WP_DEBUG`, `ABSPATH`,
`WP_CONTENT_DIR`, `WP_PLUGIN_DIR`, `FS_CHMOD_FILE`, …). `WP_DEBUG` and the
other `bool` constants already widen to `bool`; the string and integer
ones do not. Widen every constant `wp-config.php` (or
`wp-config-sample.php`) defines, rather than keeping a list of names.
