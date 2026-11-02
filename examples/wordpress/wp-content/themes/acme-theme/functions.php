<?php

declare(strict_types=1);

if (!defined('ABSPATH')) {
    exit;
}

add_action('after_setup_theme', 'acme_theme_setup');

function acme_theme_setup(): void
{
    add_theme_support('title-tag');
    add_theme_support('post-thumbnails');
    register_nav_menus(['primary' => __('Primary menu', 'acme-theme')]);
}

/**
 * The price of a product, formatted for display.
 */
function acme_theme_price(int $post_id): string
{
    // Try: hover $price. With true as the third argument get_post_meta()
    // returns the single value; without it, a list of every value stored
    // under the key.
    $price = get_post_meta($post_id, '_acme_price', true);

    return number_format_i18n((float) $price, 2);
}

/**
 * The number of the last page of the main query.
 *
 * @global WP_Query $wp_query
 */
function acme_theme_last_page(): int
{
    // Try: complete after $wp_query->. The @global tag above types it.
    global $wp_query;

    return (int) $wp_query->max_num_pages;
}

/**
 * The link to a post's term archive, or an empty string.
 */
function acme_theme_first_category_link(WP_Post $post): string
{
    // Try: hover $terms. get_the_terms() returns false when the post has
    // none, and a WP_Error for a taxonomy that does not exist.
    $terms = get_the_terms($post, 'category');
    if ($terms === false || is_wp_error($terms) || $terms === []) {
        return '';
    }

    $link = get_term_link($terms[0]);

    return is_wp_error($link) ? '' : $link;
}

// Try: go to the definition of the hook name. It jumps to where
// ProductRepository::create() in the plugin fires it. Inside the quotes,
// completion offers every hook the plugin and the theme fire or listen to.
add_action('acme_shop_product_created', 'acme_theme_forget_products');

// Try: go to the definition of this one too. The plugin fires it under a
// name it builds, 'acme_shop_saved_' . ProductPostType::NAME, which this
// name matches.
add_action('acme_shop_saved_acme_product', 'acme_theme_forget_products');

function acme_theme_forget_products(): void
{
    wp_cache_delete('acme_theme_products');
}

add_action('wp_enqueue_scripts', 'acme_theme_enqueue_assets');

function acme_theme_enqueue_assets(): void
{
    // Try: hover $dir. trailingslashit() called on a literal gives back the
    // literal WordPress returns, 'assets/'.
    $dir = trailingslashit('assets');
    wp_enqueue_style('acme-theme', get_theme_file_uri($dir . 'theme.css'), [], null);
}
