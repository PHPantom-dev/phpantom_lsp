<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;
use function stripslashes_from_strings_only;

assertType('null', stripslashes_from_strings_only(null));
assertType('true', stripslashes_from_strings_only(true));
assertType('false', stripslashes_from_strings_only(false));
assertType('bool', stripslashes_from_strings_only(Faker::bool()));
assertType('123', stripslashes_from_strings_only(123));
assertType('int', stripslashes_from_strings_only(Faker::int()));
assertType('1.23', stripslashes_from_strings_only(1.23)); // SKIP: a float literal is typed `float`, not its value
assertType('float', stripslashes_from_strings_only(Faker::float()));
assertType("''", stripslashes_from_strings_only('')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('string', stripslashes_from_strings_only('value'));
assertType('string', stripslashes_from_strings_only(Faker::string()));
assertType("array{key: 'value'}", stripslashes_from_strings_only(['key' => 'value']));
assertType('array<mixed>', stripslashes_from_strings_only(Faker::array()));
assertType('resource', stripslashes_from_strings_only(Faker::resource())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType("object{key: 'value'}&stdClass", stripslashes_from_strings_only((object)['key' => 'value'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('WP_Post', stripslashes_from_strings_only(Faker::wpPost()));
assertType('object', stripslashes_from_strings_only(Faker::object())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
