<?php

declare(strict_types=1);

namespace Acme\Shop;

use Acme\Shop\Admin\SettingsPage;

final class Plugin
{
    public function register(): void
    {
        add_action('init', [ProductPostType::class, 'register']);
        add_action('admin_menu', [new SettingsPage(), 'addMenuPage']);
        add_shortcode('acme_product', [new ProductShortcode(new ProductRepository()), 'render']);
    }
}
