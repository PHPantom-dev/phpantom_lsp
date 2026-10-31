<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_update_category;
use function PHPStan\Testing\assertType;

assertType('int<0, max>|false', wp_update_category([]));
assertType('int<0, max>|false', wp_update_category(['cat_ID' => 123]));
assertType('int<0, max>|false', wp_update_category(Faker::array()));
