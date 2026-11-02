<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_elements_class_name;
use function PHPStan\Testing\assertType;

assertType('lowercase-string&non-falsy-string', wp_get_elements_class_name());
