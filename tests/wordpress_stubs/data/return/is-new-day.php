<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function is_new_day;
use function PHPStan\Testing\assertType;

assertType('0|1', is_new_day());
