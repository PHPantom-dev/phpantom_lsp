<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_scripts_get_suffix;
use function PHPStan\Testing\assertType;

assertType("''|'.min'", wp_scripts_get_suffix(''));
assertType("''|'.min'", wp_scripts_get_suffix('dev'));
assertType("''|'.min'", wp_scripts_get_suffix('foo'));
assertType("''|'.min'", wp_scripts_get_suffix(Faker::string()));
