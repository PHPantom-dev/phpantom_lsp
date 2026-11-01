<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_permalink;
use function get_post_permalink;
use function get_the_permalink;
use function PHPStan\Testing\assertType;

// get_permalink()
assertType('string|false', get_permalink());
assertType('string|false', get_permalink(1));
assertType('string|false', get_permalink(Faker::int()));
assertType('string', get_permalink(Faker::wpPost()));

// get_the_permalink()
assertType('string|false', get_the_permalink());
assertType('string|false', get_the_permalink(1));
assertType('string|false', get_the_permalink(Faker::int()));
assertType('string', get_the_permalink(Faker::wpPost()));

// get_post_permalink()
assertType('string|false', get_post_permalink());
assertType('string|false', get_post_permalink(1));
assertType('string|false', get_post_permalink(Faker::int()));
assertType('string', get_post_permalink(Faker::wpPost()));
