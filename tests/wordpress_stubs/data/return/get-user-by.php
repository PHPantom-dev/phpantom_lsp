<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_user_by;
use function PHPStan\Testing\assertType;

assertType('WP_User|false', get_user_by('id', Faker::int()));
assertType('WP_User|false', get_user_by('id', Faker::positiveInt()));
assertType('WP_User|false', get_user_by('id', 1));
assertType('WP_User|false', get_user_by('ID', Faker::int()));
assertType('WP_User|false', get_user_by('ID', Faker::positiveInt()));
assertType('WP_User|false', get_user_by('ID', 1));

assertType('false', get_user_by('id', Faker::nonPositiveInt())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', get_user_by('id', -1)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', get_user_by('id', 0)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', get_user_by('ID', Faker::nonPositiveInt())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', get_user_by('ID', -1)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', get_user_by('ID', 0)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
