<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function check_admin_referer;
use function PHPStan\Testing\assertType;

assertType('1|2|false', check_admin_referer(-1, Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('1|2', check_admin_referer(Faker::string(), Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('1|2|false', check_admin_referer(Faker::int(), Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read
