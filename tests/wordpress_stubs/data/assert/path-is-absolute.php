<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function path_is_absolute;
use function PHPStan\Testing\assertType;

$path = Faker::string();
if (path_is_absolute($path)) {
    assertType('non-falsy-string', $path);
}
if (! path_is_absolute($path)) {
    assertType('string', $path);
}

$path = Faker::nonFalsyString();
if (path_is_absolute($path)) {
    assertType('non-falsy-string', $path);
}
if (! path_is_absolute($path)) {
    assertType('non-falsy-string', $path);
}
