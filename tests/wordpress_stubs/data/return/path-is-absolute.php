<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function path_is_absolute;
use function PHPStan\Testing\assertType;

assertType('false', path_is_absolute(''));
assertType('false', path_is_absolute('0'));
assertType('bool', path_is_absolute('foo')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('bool', path_is_absolute(Faker::string()));
