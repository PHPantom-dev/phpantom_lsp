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

assertType('false', get_user_by('id', Faker::nonPositiveInt()));
assertType('false', get_user_by('id', -1));
assertType('false', get_user_by('id', 0));
assertType('false', get_user_by('ID', Faker::nonPositiveInt()));
assertType('false', get_user_by('ID', -1));
assertType('false', get_user_by('ID', 0));
