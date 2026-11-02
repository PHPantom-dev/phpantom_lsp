<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_debug_backtrace_summary;
use function PHPStan\Testing\assertType;

assertType('string', wp_debug_backtrace_summary());
assertType('string', wp_debug_backtrace_summary(null, 0, true));
assertType('list<string>', wp_debug_backtrace_summary(null, 0, false));
