<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function trailingslashit;
use function PHPStan\Testing\assertType;

assertType("'/'", trailingslashit('')); // PHPantom folds a call on a literal to the literal WordPress returns
assertType("'value/'", trailingslashit('value')); // PHPantom folds a call on a literal to the literal WordPress returns
assertType('non-falsy-string', trailingslashit(Faker::string()));
