# PHPantom — WordPress

Known gaps and missing features in PHPantom's WordPress support. Today
WordPress gets the generic engine, plus the output constants (`OBJECT`,
`ARRAY_A`, …) its stubs leave out and the return types an argument
decides that they declare too wide (`src/wordpress/`), or that no PHPDoc
type can spell (`src/type_engine/type_ops/call_site_returns/wordpress.rs`).
Hook names navigate between the places that fire a hook and its listeners
(`src/wordpress/hooks.rs`), core's and the installed plugins' read from
their files, or core's from a list in the binary when only the stubs are
installed (`src/wordpress/hook_sources.rs`). Core's functions
and classes resolve only when the project indexes WordPress itself or a
stub package for it. The stubs' own type tests run in
`tests/wordpress_stubs/`, and what the engine reads wrongly in them is
filed under "Conditional return types" in [bugs.md](bugs.md). The
return types `src/wordpress/overrides.php` gives the stubs that hold for
every WordPress version the stubs cover are worth offering to
`php-stubs/wordpress-stubs` as well.

Most WordPress typing lives in `php-stubs/wordpress-stubs`, so the
external stubs are the foundation for everything here. Installed with
Composer, the package is a stub source with no configuration, as is
`php-stubs/woocommerce-stubs`; a project without Composer can list
either under `[stubs] paths`. `global $wpdb;` takes its type from a
`@global` tag on the function, else from the `@global` tags of the stubs,
else from those of core and any other indexed code.
PHPStan's WordPress extension
(`szepeviktor/phpstan-wordpress`) is the checklist of what a WordPress
developer expects from types; PhpStorm's built-in WordPress support and
PHP Tools' hook navigation are what they expect from an editor.

`examples/wordpress/` is the demo project: a plugin and a theme with core
from the stubs, and an `assertions.php` that checks their claims against
WordPress installed on SQLite. The real-world baseline is Contact Form 7
(a plugin) and GeneratePress (a classic theme), analysed with the stubs
installed. Of their 104 and 232 diagnostics (2026-11-02), most are what
the stubs or the projects' own docblocks declare, and PHPStan reports
them too. What they show PHPantom getting wrong is filed below under
WP3, WP9 and WP15. Hooks and template parts produce no diagnostics
today, so the baseline cannot rank WP5 and WP6; they rank by what an
editor user compares first.

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

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

The baseline also found parameters the stubs declare narrower than core
accepts. `wp_hash()` takes `'auth'|'logged_in'|'nonce'|'secure_auth'` as
its scheme, but `wp_salt()` makes a salt for any scheme, and Contact
Form 7 hashes with its own. `the_title_attribute()` and its siblings take
only an array shape, though core runs `wp_parse_args()` on a query string
like `'echo=0'` too. `WP_Customize_Manager::add_control()` and
`add_section()` close their `$args` shapes, but the control or section
class reads keys of its own (GeneratePress passes `pro_text` and
`pro_url`), and `settings` is a setting ID as often as an array. Correct
these in `src/wordpress/overrides.php`, which replaces only return types
today, and offer them to the stubs.

---

## WP15. `analyze` leaves out a plugin's files outside its PSR-4 roots

**Impact: Medium · Complexity: Low**

```json
{ "autoload": { "psr-4": { "Acme\\Shop\\": "src/" } } }
```

A plugin that autoloads its classes with Composer keeps the rest of its
code outside the PSR-4 root: the main file with the `Plugin Name:`
header, `uninstall.php`, `templates/`, `views/`. `analyze` walks only
the PSR-4 roots when a `composer.json` declares any, so none of those
files are checked, and a theme next to the plugin is skipped as a whole.
The editor still reports them when they are open. Analyse a plugin's or
theme's whole directory (one holding a file with a `Plugin Name:` or
`Theme Name:` header, minus `vendor/` and `node_modules/`), the way
Blade view directories are added today.

**Where to look:** `discover_user_files` in `src/analyse/run.rs`.

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

The hooks core and the sibling plugins fire belong with them: today
`src/wordpress/hook_sources.rs` reads the hooks of the WordPress packages
Composer installs, and falls back to its list of core's hook names, so a
plugin opened on its own completes core's hooks but cannot go to them.

This follows the rule in `external-stubs.md`: Drupal's web root and
sibling editor folders are detected, never typed by the user, and every
new source of PHP maps onto an existing role.

Contact Form 7 shows what a sibling plugin that is not installed costs
today. It calls Akismet behind `is_callable(['Akismet', 'get_api_key'])`,
which still reports `Akismet` as an unknown class, and Really Simple
CAPTCHA behind `class_exists('ReallySimpleCaptcha')`, which is accepted
but leaves 18 members "could not be resolved" on the object it builds.

**Where to look:** `src/classmap_scanner/discovery.rs`
(`scan_drupal_directories` is the closest precedent),
`src/config/indexing.rs`.

---

## WP5. Hook callback parameter and return types

**Impact: Medium-High · Complexity: Medium-High**

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
`apply_filters()` returns. With a callback linked to its hook by name, the
closure parameter inference that already feeds `array_map()` callbacks
can feed hook callbacks from the same docblock.

PHPStan's WordPress extension also checks a callback against the hook:
too few accepted arguments for the parameters it declares (the fourth
argument of `add_action()`), and a filter callback that returns nothing.
Those are good diagnostics once the types are there.

A project with only the stubs has no core to read the docblocks from: the
list of core's hooks in `src/wordpress/core_hooks.txt` holds names only.
The hook list `wp-hooks/wordpress-core` publishes has every core hook with
its docblock, and could take its place.

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

---

## WP14. `wp_parse_args()` of an object

**Impact: Low · Complexity: Medium**

```php
$args = wp_parse_args($post, ['extra' => 1]);   // array, want an array with an `extra` key
```

`wp_parse_args()` reads an object through `get_object_vars()`, called
from inside WordPress, so the result holds the object's public
properties merged under the defaults. PHPStan's WordPress extension
types it `non-empty-array<mixed>&hasOffset('extra')` outside a class,
and keeps the declared `array` inside one, where the caller sees more
properties than WordPress does. PHPantom keeps the declared `array`
everywhere: it has no `hasOffset()` type, and does not type
`get_object_vars()` by the scope it is called from. The fixture is
ported in `tests/wordpress_stubs/data/phpstan-wordpress/`, marked
`// SKIP`.

---

## WP16. Benchmark for WordPress hook names and stubs

**Impact: Low-Medium · Complexity: Low**

Laravel, Symfony, and Yii have Criterion benchmarks in `benches/`;
WordPress has none, though its hook names are read from every installed
plugin's files and its stubs are one 6 MB file. Add
`benches/wordpress_completion.rs`: generate a plugin that fires and
listens to a few hundred hooks across many files, and time hook name
completion inside `add_action('…')` and `apply_filters('…')`,
go-to-definition from a listener to the firing, and member completion
on a value whose type comes from the stubs (`get_post()`, `$wpdb`).
Register it in `Cargo.toml` so `benches/ci_benchmarks.py` picks it up.
