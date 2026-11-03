<?php

declare(strict_types=1);

namespace Acme\Shop;

use WP_Post;

final readonly class Product
{
    public function __construct(
        public int $id,
        public string $title,
        public string $sku,
        public float $price,
    ) {}

    public static function fromPost(WP_Post $post): self
    {
        // Try: complete after $post->. The properties come from the stubs'
        // WP_Post class.
        return new self(
            $post->ID,
            $post->post_title,
            (string) get_post_meta($post->ID, '_acme_sku', true),
            (float) get_post_meta($post->ID, '_acme_price', true),
        );
    }
}
