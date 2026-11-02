<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function addslashes_gpc;
use function PHPStan\Testing\assertType;

assertType('string', addslashes_gpc(''));
assertType('string', addslashes_gpc('value'));
assertType('string', addslashes_gpc(Faker::nonEmptyString()));
assertType('string', addslashes_gpc(Faker::string()));

assertType('array', addslashes_gpc([]));
assertType('array', addslashes_gpc(['key' => 'value']));
assertType('array', addslashes_gpc(Faker::nonEmptyArray()));
assertType('array', addslashes_gpc(Faker::array()));
