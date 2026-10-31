<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function add_cssclass;
use function PHPStan\Testing\assertType;

assertType("''", add_cssclass('', '')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType("'foo'", add_cssclass('foo', '')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('non-empty-string', add_cssclass('', 'foo'));
assertType('non-empty-string', add_cssclass('foo', 'bar'));
assertType('string', add_cssclass(Faker::string(), Faker::string()));
assertType('non-falsy-string', add_cssclass(Faker::nonFalsyString(), '')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('non-empty-string', add_cssclass(Faker::nonFalsyString(), Faker::nonFalsyString())); // SKIP: a type and its own subtype are not collapsed in a union or an intersection
assertType('non-empty-string', add_cssclass(Faker::nonFalsyString(), Faker::string())); // SKIP: a type and its own subtype are not collapsed in a union or an intersection
