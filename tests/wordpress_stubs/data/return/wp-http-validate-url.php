<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_http_validate_url;
use function PHPStan\Testing\assertType;

assertType('false', wp_http_validate_url(''));
assertType('false', wp_http_validate_url('1')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('false', wp_http_validate_url(Faker::numericString())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

assertType("'url'|false", wp_http_validate_url('url'));

assertType('non-empty-string|false', wp_http_validate_url(Faker::string())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('non-empty-string|false', wp_http_validate_url(Faker::nonEmptyString()));
assertType('non-falsy-string|false', wp_http_validate_url(Faker::nonFalsyString()));
assertType('(lowercase-string&non-empty-string)|false', wp_http_validate_url(Faker::lowercaseString())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('(lowercase-string&non-falsy-string)|false', wp_http_validate_url(Faker::intersection(Faker::lowercaseString(), Faker::nonFalsyString()))); // SKIP: a conditional over a method template ignores its bound and does not narrow it
