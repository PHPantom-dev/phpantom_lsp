<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_script_tag;
use function wp_get_inline_script_tag;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', wp_get_script_tag([]));
assertType('non-falsy-string', wp_get_script_tag(['key' => 'value']));
assertType('non-falsy-string', wp_get_script_tag(Faker::strArray(Faker::string())));

assertType('non-falsy-string', wp_get_inline_script_tag(Faker::string(), []));
assertType('non-falsy-string', wp_get_inline_script_tag(Faker::string(), ['key' => 'value']));
assertType('non-falsy-string', wp_get_inline_script_tag(Faker::string(), Faker::strArray(Faker::string())));
