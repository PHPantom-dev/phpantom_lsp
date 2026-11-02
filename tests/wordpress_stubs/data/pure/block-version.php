<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function block_version;
use function PHPStan\Testing\assertType;

$content = Faker::string();

assertType('0|1', block_version($content));

if (block_version($content) === 1) {
    assertType('1', block_version($content));
}

if (block_version($content) === 0) {
    assertType('0', block_version($content));
}
