<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function validate_plugin;
use function PHPStan\Testing\assertType;

assertType('WP_Error', validate_plugin(''));
assertType('0|WP_Error', validate_plugin('pluginPath'));
assertType('0|WP_Error', validate_plugin(Faker::string()));
