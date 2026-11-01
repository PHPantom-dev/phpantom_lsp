<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_plugin_page_hookname;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', get_plugin_page_hookname('', ''));
assertType('non-falsy-string', get_plugin_page_hookname('plugin_page', 'parent_page'));
assertType('non-falsy-string', get_plugin_page_hookname(Faker::string(), Faker::string()));
