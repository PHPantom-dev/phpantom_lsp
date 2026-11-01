<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_die;
use function PHPStan\Testing\assertType;

// default (['exit' => true])
assertType('never', wp_die());
assertType('never', wp_die(''));
assertType('never', wp_die('', ''));

// explicit exit
assertType('never', wp_die('', '', ['exit' => true]));
assertType('null', wp_die('', '', ['exit' => false]));

// unknown
assertType('null', wp_die('', '', ['exit' => Faker::bool()]));
assertType('null', wp_die('', '', Faker::array()));

// non-array $args parameter ($args not string type per @phpstan-param)
assertType('never', wp_die('', '', Faker::int()));
