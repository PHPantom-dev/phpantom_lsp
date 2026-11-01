<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function taxonomy_exists;
use function PHPStan\Testing\assertType;

assertType('false', taxonomy_exists(''));
assertType('false', taxonomy_exists('0'));
assertType('bool', taxonomy_exists('tax')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('bool', taxonomy_exists(Faker::string()));
assertType('bool', taxonomy_exists(Faker::nonEmptyString()));
assertType('bool', taxonomy_exists(Faker::nonFalsyString()));
