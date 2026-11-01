<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function mysql2date;
use function PHPStan\Testing\assertType;

$time = '1970-01-01 00:00:00';

// Constant strings
assertType('int|false', mysql2date('G', $time));
assertType('int|false', mysql2date('U', $time));
assertType('string|false', mysql2date('Y-m-d H:i:s', $time));

// Unknown string
assertType('int|string|false', mysql2date(Faker::string(), $time));
