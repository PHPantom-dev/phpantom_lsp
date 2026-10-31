<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function add_cssclass;
use function PHPStan\Testing\assertType;

$classes = Faker::string();
$classToAdd = Faker::string();

assertType('string', add_cssclass($classes, $classToAdd));

if (add_cssclass($classes, $classToAdd) === 'foo') {
    assertType("'foo'", add_cssclass($classes, $classToAdd));
}

if (add_cssclass($classes, $classToAdd) === 'bar') {
    assertType("'bar'", add_cssclass($classes, $classToAdd));
}
