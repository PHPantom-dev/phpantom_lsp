<?php

declare(strict_types=1);

namespace Acme\Shop;

use WP_Error;
use WP_Post_Type;

final class ProductPostType
{
    public const NAME = 'acme_product';

    public static function register(): WP_Post_Type|WP_Error
    {
        // Try: hover register_post_type(), or go to its definition. Its
        // declaration comes from php-stubs/wordpress-stubs in vendor/.
        return register_post_type(self::NAME, [
            'label' => __('Products', 'acme-shop'),
            'public' => true,
            'supports' => ['title', 'editor', 'excerpt', 'custom-fields'],
        ]);
    }
}
