<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_internal_hosts;
use function PHPStan\Testing\assertType;

assertType('array<lowercase-string>', wp_internal_hosts());
