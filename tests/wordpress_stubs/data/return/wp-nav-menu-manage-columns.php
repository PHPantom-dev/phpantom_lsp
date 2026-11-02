<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_nav_menu_manage_columns;
use function PHPStan\Testing\assertType;

assertType("array{_title: string, cb: '<input type=\"checkbox\" />', link-target: string, title-attribute: string, css-classes: string, xfn: string, description: string}", wp_nav_menu_manage_columns());
