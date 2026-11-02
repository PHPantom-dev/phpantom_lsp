<?php

declare(strict_types=1);

namespace Acme\Shop\Admin;

final class SettingsPage
{
    public const OPTION = 'acme_shop_settings';

    public function addMenuPage(): void
    {
        add_menu_page(
            __('Acme Shop', 'acme-shop'),
            __('Acme Shop', 'acme-shop'),
            'manage_options',
            'acme-shop',
            [$this, 'render'],
        );
    }

    public function render(): void
    {
        if (!current_user_can('manage_options')) {
            // wp_die() never returns, so nothing after this block runs for a
            // user without the capability.
            wp_die(esc_html__('You may not manage the shop.', 'acme-shop'));
        }

        $currency = (string) get_option(self::OPTION . '_currency', 'EUR');
        $title = get_admin_page_title();

        // views/settings-page.php reads $currency and $title, and declares
        // them with @var.
        require __DIR__ . '/../../views/settings-page.php';
    }
}
