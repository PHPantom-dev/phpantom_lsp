<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

assertType('non-falsy-string', Faker::wpWidget()->get_field_id('field_name'));
assertType('non-falsy-string', Faker::wpWidget()->get_field_name('field_name'));
