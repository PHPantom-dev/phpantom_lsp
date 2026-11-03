<?php

/**
 * Plugin Name: Acme Shop
 * Description: The plugin half of PHPantom's WordPress demo.
 * Version: 1.0.0
 * Requires PHP: 8.2
 * License: MIT
 */

declare(strict_types=1);

use Acme\Shop\Plugin;

if (!defined('ABSPATH')) {
    exit;
}

// The classes under src/ are autoloaded by the composer.json at the root of
// the demo, the way a Composer-managed site (such as Bedrock) loads them.
// A plugin that ships its own vendor/ would require its autoloader here.

(new Plugin())->register();
