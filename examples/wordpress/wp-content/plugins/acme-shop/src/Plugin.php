<?php

declare(strict_types=1);

namespace Acme\Shop;

use Acme\Shop\Admin\SettingsPage;

final class Plugin
{
    public function register(): void
    {
        // Try: go to the definition of 'init'. Core is installed with
        // Composer, so it jumps to the do_action() in core's wp-settings.php.
        // Complete inside the quotes for every hook core fires.
        add_action('init', [ProductPostType::class, 'register']);
        add_action('admin_menu', [new SettingsPage(), 'addMenuPage']);
        add_shortcode('acme_product', [new ProductShortcode(new ProductRepository()), 'render']);
    }
}
