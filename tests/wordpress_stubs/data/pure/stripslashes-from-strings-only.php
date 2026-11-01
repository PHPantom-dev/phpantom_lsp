<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function stripslashes_from_strings_only;
use function PHPStan\Testing\assertType;

$value = Faker::string();
if (stripslashes_from_strings_only($value) === 'foo') {
    assertType("'foo'", stripslashes_from_strings_only($value));
}

$value = Faker::int();
if (stripslashes_from_strings_only($value) === 123) {
    assertType('123', stripslashes_from_strings_only($value));
}
