<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function trailingslashit;
use function PHPStan\Testing\assertType;

assertType('lowercase-string&non-falsy-string', trailingslashit('')); // PHPantom's override keeps a lowercase string lowercase
assertType('lowercase-string&non-falsy-string', trailingslashit('value')); // PHPantom's override keeps a lowercase string lowercase
assertType('non-falsy-string', trailingslashit(Faker::string()));
