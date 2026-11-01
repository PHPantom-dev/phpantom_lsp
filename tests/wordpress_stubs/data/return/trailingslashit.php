<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function trailingslashit;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', trailingslashit(''));
assertType('non-falsy-string', trailingslashit('value'));
assertType('non-falsy-string', trailingslashit(Faker::string()));
