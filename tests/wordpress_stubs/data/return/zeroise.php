<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function zeroise;
use function PHPStan\Testing\assertType;

// $threshold is 0
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(-1, 0));
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(0, 0));
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(1, 0));
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(Faker::int(), 0));

// $threshold > 0
assertType('lowercase-string&non-empty-string', zeroise(-1, 5));
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(0, 5));
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(1, 5));
assertType('lowercase-string&non-empty-string', zeroise(Faker::int(), 5));

// $threshold is unknown
assertType('lowercase-string&non-empty-string', zeroise(-1, Faker::nonNegativeInt()));
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(0, Faker::nonNegativeInt()));
assertType('lowercase-string&non-empty-string&numeric-string', zeroise(1, Faker::nonNegativeInt()));
assertType('lowercase-string&non-empty-string', zeroise(Faker::int(), Faker::nonNegativeInt()));
