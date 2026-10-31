<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function delete_plugins;
use function PHPStan\Testing\assertType;

$plugins = Faker::array();

assertType('bool|WP_Error|null', delete_plugins($plugins));

if ($plugins !== []) {
    assertType('WP_Error|true|null', delete_plugins($plugins)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
}

if ($plugins === []) {
    assertType('false', delete_plugins($plugins)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
}
