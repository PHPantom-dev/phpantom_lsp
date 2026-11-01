<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_robots_max_image_preview_large;
use function wp_robots_no_robots;
use function wp_robots_noindex;
use function wp_robots_noindex_embeds;
use function wp_robots_noindex_search;
use function wp_robots_sensitive_page;
use function PHPStan\Testing\assertType;

assertType('array<string, bool|string>', wp_robots_max_image_preview_large(Faker::array()));
assertType('array<string, bool|string>', wp_robots_no_robots(Faker::array()));
assertType('array<string, bool|string>', wp_robots_noindex(Faker::array()));
assertType('array<string, bool|string>', wp_robots_noindex_embeds(Faker::array()));
assertType('array<string, bool|string>', wp_robots_noindex_search(Faker::array()));
assertType('array<string, bool|string>', wp_robots_sensitive_page(Faker::array()));
