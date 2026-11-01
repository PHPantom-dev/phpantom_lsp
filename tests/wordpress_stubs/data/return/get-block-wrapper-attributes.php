<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_block_wrapper_attributes;
use function PHPStan\Testing\assertType;

assertType('string', get_block_wrapper_attributes());
assertType('string', get_block_wrapper_attributes([]));
assertType('non-falsy-string', get_block_wrapper_attributes(['class' => 'my-class']));

assertType('non-falsy-string', get_block_wrapper_attributes(Faker::nonEmptyArray(Faker::string(), Faker::string())));
assertType('string', get_block_wrapper_attributes(Faker::strArray()));
