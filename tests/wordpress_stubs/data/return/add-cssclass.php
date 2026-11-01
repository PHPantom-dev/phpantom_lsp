<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function add_cssclass;
use function PHPStan\Testing\assertType;

assertType("''", add_cssclass('', ''));
assertType("'foo'", add_cssclass('foo', ''));
assertType('non-empty-string', add_cssclass('', 'foo'));
assertType('non-empty-string', add_cssclass('foo', 'bar'));
assertType('string', add_cssclass(Faker::string(), Faker::string()));
assertType('non-falsy-string', add_cssclass(Faker::nonFalsyString(), ''));
assertType('non-empty-string', add_cssclass(Faker::nonFalsyString(), Faker::nonFalsyString()));
assertType('non-empty-string', add_cssclass(Faker::nonFalsyString(), Faker::string()));
