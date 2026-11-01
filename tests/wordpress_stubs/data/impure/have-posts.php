<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function have_posts;
use function PHPStan\Testing\assertType;

assertType('bool', have_posts());

if (have_posts()) {
    assertType('bool', have_posts());
}

if (! have_posts()) {
    assertType('bool', have_posts());
}
