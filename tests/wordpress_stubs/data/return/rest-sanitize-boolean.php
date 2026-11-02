<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function rest_sanitize_boolean;
use function PHPStan\Testing\assertType;

assertType('true', rest_sanitize_boolean(true));
assertType('false', rest_sanitize_boolean(false));
assertType('bool', rest_sanitize_boolean(Faker::bool()));

assertType('false', rest_sanitize_boolean(''));
assertType('false', rest_sanitize_boolean('0'));
assertType('false', rest_sanitize_boolean('false'));
assertType('false', rest_sanitize_boolean('FALSE'));
assertType('true', rest_sanitize_boolean('value'));
assertType('bool', rest_sanitize_boolean(Faker::string()));
assertType('bool', rest_sanitize_boolean(Faker::nonEmptyString()));
assertType('bool', rest_sanitize_boolean(Faker::nonFalsyString()));

// See https://github.com/php-stubs/wordpress-stubs/pull/346#issuecomment-3181779716
assertType('true', rest_sanitize_boolean('False'));
assertType('true', rest_sanitize_boolean('foo'));

assertType('false', rest_sanitize_boolean(0));
assertType('true', rest_sanitize_boolean(123));
assertType('bool', rest_sanitize_boolean(Faker::int()));
assertType('true', rest_sanitize_boolean(Faker::positiveInt()));
assertType('bool', rest_sanitize_boolean(Faker::nonNegativeInt()));
