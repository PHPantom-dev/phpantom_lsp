<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_site_screen_help_sidebar_content;
use function get_site_screen_help_tab_args;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', get_site_screen_help_sidebar_content());

assertType("array{id: 'overview', title: string, content: non-falsy-string}", get_site_screen_help_tab_args());
