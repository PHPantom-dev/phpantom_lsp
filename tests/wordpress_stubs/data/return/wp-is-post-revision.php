<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_is_post_revision;
use function PHPStan\Testing\assertType;

assertType('false', wp_is_post_revision(0));
assertType('false', wp_is_post_revision(Faker::nonPositiveInt()));
assertType('int<0, max>|false', wp_is_post_revision(123));
assertType('int<0, max>|false', wp_is_post_revision(Faker::int()));
assertType('int<0, max>|false', wp_is_post_revision(Faker::positiveInt()));
assertType('int<0, max>|false', wp_is_post_revision(Faker::wpPost()));
