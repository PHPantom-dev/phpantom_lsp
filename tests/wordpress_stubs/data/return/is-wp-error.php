<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function is_wp_error;
use function PHPStan\Testing\assertType;

assertType('false', is_wp_error(Faker::string()));
assertType('true', is_wp_error(Faker::wpError()));
assertType('bool', is_wp_error(Faker::mixed()));
