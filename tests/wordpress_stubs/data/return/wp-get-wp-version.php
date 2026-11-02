<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_wp_version;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', wp_get_wp_version());
