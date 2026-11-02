<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function taxonomy_exists;
use function PHPStan\Testing\assertType;

$taxonomy = Faker::string();
if (taxonomy_exists($taxonomy)) {
    assertType('non-falsy-string', $taxonomy);
}
if (! taxonomy_exists($taxonomy)) {
    assertType('string', $taxonomy);
}
