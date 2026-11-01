<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

$wpQuery = Faker::wpQuery();

assertType('bool', $wpQuery->have_posts());

if ($wpQuery->have_posts()) {
    assertType('bool', $wpQuery->have_posts());
}

if (! $wpQuery->have_posts()) {
    assertType('bool', $wpQuery->have_posts());
}
