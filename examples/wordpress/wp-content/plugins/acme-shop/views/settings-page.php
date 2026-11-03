<?php

declare(strict_types=1);

/**
 * Included by Acme\Shop\Admin\SettingsPage::render().
 *
 * @var string $currency
 * @var string $title
 */

?>
<div class="wrap">
    <h1><?php echo esc_html($title); ?></h1>
    <p><?php echo esc_html(sprintf(__('Prices are shown in %s.', 'acme-shop'), $currency)); ?></p>
</div>
