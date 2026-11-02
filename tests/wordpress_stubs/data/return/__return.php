<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function __return_empty_array;
use function __return_empty_string;
use function __return_zero;
use function PHPStan\Testing\assertType;

assertType('0', __return_zero());
assertType('array{}', __return_empty_array());
assertType("''", __return_empty_string());
