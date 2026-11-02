<?php

/**
 * WordPress Demo Assertions
 *
 * Run: php examples/wordpress/assertions.php
 *
 * These assertions install WordPress from vendor/ into a throwaway SQLite
 * database, load the demo plugin and theme, and check that every claim
 * their "Try:" comments make holds at runtime. PHPantom reads the stubs
 * statically, so a type that only it believes in is a bug in the demo (or
 * in the stubs). Requires the pdo_sqlite extension, but no web server or
 * MySQL.
 */

declare(strict_types=1);

require_once __DIR__ . '/vendor/autoload.php';

use Acme\Shop\Admin\SettingsPage;
use Acme\Shop\Product;
use Acme\Shop\ProductPostType;
use Acme\Shop\ProductRepository;
use Acme\Shop\ProductShortcode;

$passed = 0;
$failed = 0;

function check(string $label, bool $condition): void
{
    global $passed, $failed;
    if ($condition) {
        $passed++;
    } else {
        $failed++;
        echo "FAIL: $label\n";
    }
}

/**
 * Lay out a content directory in a temporary folder: the demo's plugins and
 * themes linked in, and a `db.php` drop-in that loads the SQLite driver.
 * The database file lands there too, and the folder is removed on exit.
 */
function createContentDirectory(): string
{
    $dir = sys_get_temp_dir() . '/phpantom-wordpress-demo-' . getmypid();
    mkdir($dir . '/database', 0777, true);
    symlink(__DIR__ . '/wp-content/plugins', $dir . '/plugins');
    symlink(__DIR__ . '/wp-content/themes', $dir . '/themes');
    $driver = __DIR__ . '/vendor/wpackagist-plugin/sqlite-database-integration/wp-includes/sqlite/db.php';
    file_put_contents($dir . '/db.php', '<?php require_once ' . var_export($driver, true) . ";\n");

    register_shutdown_function(static function () use ($dir): void {
        $files = new RecursiveIteratorIterator(
            new RecursiveDirectoryIterator($dir, FilesystemIterator::SKIP_DOTS),
            RecursiveIteratorIterator::CHILD_FIRST,
        );
        foreach ($files as $file) {
            /** @var SplFileInfo $file */
            $file->isDir() && !$file->isLink() ? rmdir($file->getPathname()) : unlink($file->getPathname());
        }
        rmdir($dir);
    });

    return $dir;
}

// ── Boot ────────────────────────────────────────────────────────────────────

$contentDir = createContentDirectory();

define('ABSPATH', __DIR__ . '/vendor/roots/wordpress-no-content/');
define('WP_CONTENT_DIR', $contentDir);
define('WP_INSTALLING', true);
define('DB_ENGINE', 'sqlite');
define('DB_DIR', $contentDir . '/database/');
define('DB_FILE', '.ht.sqlite');
define('DB_NAME', 'wordpress');
define('DB_USER', '');
define('DB_PASSWORD', '');
define('DB_HOST', '');
$table_prefix = 'wp_';
$_SERVER['HTTP_HOST'] = 'example.test';
$_SERVER['SERVER_NAME'] = 'example.test';

require_once ABSPATH . 'wp-settings.php';
require_once ABSPATH . 'wp-admin/includes/upgrade.php';

// Installing mails the password to the admin; there is no mailer to send it.
add_filter('pre_wp_mail', '__return_true');
wp_install('Acme', 'admin', 'admin@example.test', false, '', 'password');
wp_set_current_user(1);

require_once WP_CONTENT_DIR . '/plugins/acme-shop/acme-shop.php';
require_once WP_CONTENT_DIR . '/themes/acme-theme/functions.php';

// wp_die() exits by default; throw instead, so a call that never returns
// can be observed.
add_filter('wp_die_handler', static fn(): Closure => static function (): never {
    throw new RuntimeException('wp_die');
});

// ── The stubs' constants and globals ────────────────────────────────────────

check('OBJECT is defined', defined('OBJECT'));
check('ARRAY_A is defined', defined('ARRAY_A'));
check('$wpdb is a wpdb', $GLOBALS['wpdb'] instanceof wpdb);
check('$wp_query is a WP_Query', $GLOBALS['wp_query'] instanceof WP_Query);

// ── Plugin: Plugin ──────────────────────────────────────────────────────────

check(
    'core fires the init hook the plugin listens to',
    has_action('init', [ProductPostType::class, 'register']) !== false && did_action('init') > 0,
);

// ── Plugin: ProductPostType ─────────────────────────────────────────────────

check('register_post_type() returns a WP_Post_Type', ProductPostType::register() instanceof WP_Post_Type);

// ── Plugin: ProductRepository ───────────────────────────────────────────────

$repository = new ProductRepository();

$id = $repository->create('Anvil', 'SKU-1', 12.5);
check('create() returns a positive int', is_int($id) && $id > 0);
assert(is_int($id));
update_post_meta($id, '_acme_stock', 5);

check(
    'create() with nothing to save returns a WP_Error',
    $repository->create('', 'SKU-2', 1.0) instanceof WP_Error,
);

check('get_post() returns a WP_Post', get_post($id) instanceof WP_Post);
check('get_post() of a missing ID returns null', get_post(PHP_INT_MAX) === null);
$postArray = get_post($id, ARRAY_A);
check('get_post(…, ARRAY_A) returns an array', is_array($postArray) && ($postArray['ID'] ?? null) === $id);

check('get_post_meta(…, true) returns the single value', get_post_meta($id, '_acme_sku', true) === 'SKU-1');
check('get_post_meta() without true returns a list', get_post_meta($id, '_acme_sku') === ['SKU-1']);

$product = $repository->find($id);
check('find() returns a Product', $product instanceof Product);
check('Product reads the title', $product?->title === 'Anvil');
check('Product reads the SKU meta', $product?->sku === 'SKU-1');
check('Product reads the price meta', $product?->price === 12.5);
check('find() of a missing ID returns null', $repository->find(PHP_INT_MAX) === null);

check(
    'wp_parse_args() keeps the keys of the defaults',
    wp_parse_args(['per_page' => 3], ['per_page' => 10, 'order' => 'DESC']) === ['per_page' => 3, 'order' => 'DESC'],
);

$repository->create('Rocket skates', 'SKU-3', 99.0);
$latest = $repository->latest(['per_page' => 1]);
check('latest() honours per_page', count($latest) === 1);
check('latest() returns a list of Product', array_is_list($latest) && $latest[0] instanceof Product);

$ids = $repository->ids();
check('ids() returns post IDs', count($ids) === 2 && array_filter($ids, is_int(...)) === $ids);

check('countInStock() counts the stock meta', $repository->countInStock() === 1);

// ── Plugin: ProductShortcode ────────────────────────────────────────────────

check(
    'shortcode_atts() keeps exactly the keys of the defaults',
    array_keys(shortcode_atts(['id' => '0', 'show_price' => 'yes'], ['id' => '7', 'colour' => 'red'])) === ['id', 'show_price'],
);
$html = do_shortcode('[acme_product id="' . $id . '"]');
check('[acme_product] renders the title', str_contains($html, 'Anvil'));
check('[acme_product] renders the price', str_contains($html, '12.50'));
check(
    '[acme_product show_price="no"] leaves the price out',
    !str_contains(do_shortcode('[acme_product id="' . $id . '" show_price="no"]'), '12.50'),
);
check('render() accepts the empty string WordPress passes', (new ProductShortcode($repository))->render('') === '');

// ── Plugin: SettingsPage ────────────────────────────────────────────────────

ob_start();
(new SettingsPage())->render();
check('the settings page reads the currency', str_contains((string) ob_get_clean(), 'EUR'));

wp_set_current_user(0);
ob_start();
try {
    (new SettingsPage())->render();
    $returned = true;
} catch (RuntimeException) {
    $returned = false;
}
ob_end_clean();
check('wp_die() does not return', !$returned);
wp_set_current_user(1);

// ── Theme ───────────────────────────────────────────────────────────────────

check('acme_theme_price() formats the price meta', acme_theme_price($id) === '12.50');
check(
    'the theme listens to the hook create() fires',
    has_action('acme_shop_product_created', 'acme_theme_forget_products') !== false
        && did_action('acme_shop_product_created') > 0,
);
check(
    'the name create() builds is the one the theme listens to',
    has_action('acme_shop_saved_acme_product', 'acme_theme_forget_products') !== false
        && did_action('acme_shop_saved_acme_product') > 0,
);
check('acme_theme_last_page() returns an int', is_int(acme_theme_last_page()));
check('trailingslashit() appends one slash', trailingslashit('assets') === 'assets/');

check('get_the_terms() of an unknown taxonomy is a WP_Error', get_the_terms($id, 'acme_nonexistent') instanceof WP_Error);
check('get_the_terms() of a post without terms is false', get_the_terms($id, 'category') === false);

$postId = wp_insert_post(['post_title' => 'Hello', 'post_status' => 'publish']);
$post = get_post($postId);
check('a post gets a category', $post instanceof WP_Post && is_array(get_the_terms($post, 'category')));
check(
    'acme_theme_first_category_link() links the category',
    $post instanceof WP_Post && str_contains(acme_theme_first_category_link($post), 'example.test'),
);

echo "\n$passed passed, $failed failed\n";
exit($failed > 0 ? 1 : 0);
