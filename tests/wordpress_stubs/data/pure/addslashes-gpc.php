<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function addslashes_gpc;
use function PHPStan\Testing\assertType;

$gpc = Faker::string();
if (addslashes_gpc($gpc) === 'foo') {
    assertType("'foo'", addslashes_gpc($gpc));
}

$gpc = Faker::array();
if (addslashes_gpc($gpc) === ['foo' => 'bar']) {
    assertType("array{foo: 'bar'}", addslashes_gpc($gpc));
}
