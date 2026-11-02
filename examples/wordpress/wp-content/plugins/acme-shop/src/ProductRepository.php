<?php

declare(strict_types=1);

namespace Acme\Shop;

use WP_Error;

final class ProductRepository
{
    public function find(int $id): ?Product
    {
        // Try: hover $post. get_post() returns a WP_Post or null, and
        // passing ARRAY_A instead makes it an array of the post's fields.
        $post = get_post($id);
        if ($post === null || $post->post_type !== ProductPostType::NAME) {
            return null;
        }

        return Product::fromPost($post);
    }

    /**
     * @param array{per_page?: int, order?: 'ASC'|'DESC'} $args
     * @return list<Product>
     */
    public function latest(array $args = []): array
    {
        // Try: hover $args after this line, or complete inside $args['']. The
        // result has the keys of the defaults, typed by what the caller may
        // pass for each, so 'per_page' is an int.
        $args = wp_parse_args($args, ['per_page' => 10, 'order' => 'DESC']);

        // Try: hover $posts. Without a 'fields' argument get_posts() returns
        // posts.
        $posts = get_posts([
            'post_type' => ProductPostType::NAME,
            'posts_per_page' => $args['per_page'],
            'order' => $args['order'],
        ]);

        return array_values(array_map(Product::fromPost(...), $posts));
    }

    /**
     * @return array<int, int>
     */
    public function ids(): array
    {
        // Try: hover the call. 'fields' => 'ids' makes get_posts() return
        // post IDs instead of posts.
        return get_posts(['post_type' => ProductPostType::NAME, 'fields' => 'ids']);
    }

    public function create(string $title, string $sku, float $price): int|WP_Error
    {
        // Try: hover $id. Passing true as the second argument makes
        // wp_insert_post() report a failure as a WP_Error instead of 0.
        $id = wp_insert_post([
            'post_type' => ProductPostType::NAME,
            'post_title' => $title,
            'post_status' => 'publish',
        ], true);
        if (is_wp_error($id)) {
            return $id;
        }

        // Try: hover $id again. is_wp_error() ruled the error out above, so
        // it is a positive int here.
        update_post_meta($id, '_acme_sku', $sku);
        update_post_meta($id, '_acme_price', $price);

        return $id;
    }

    /**
     * @global \wpdb $wpdb
     */
    public function countInStock(): int
    {
        // Try: complete after $wpdb->. The @global tag above types the
        // global this method imports.
        global $wpdb;

        return (int) $wpdb->get_var($wpdb->prepare(
            'SELECT COUNT(*) FROM %i WHERE meta_key = %s AND meta_value > 0',
            $wpdb->postmeta,
            '_acme_stock',
        ));
    }
}
