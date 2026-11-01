<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_user;
use function PHPStan\Testing\assertType;

assertType('WP_User|false', get_user(Faker::int()));
assertType('WP_User|false', get_user(Faker::positiveInt()));
assertType('WP_User|false', get_user(1));

assertType('false', get_user(Faker::nonPositiveInt())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', get_user(-1)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', get_user(0)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
