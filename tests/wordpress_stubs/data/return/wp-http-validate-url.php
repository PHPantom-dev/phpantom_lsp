<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_http_validate_url;
use function PHPStan\Testing\assertType;

assertType('false', wp_http_validate_url(''));
assertType('false', wp_http_validate_url('1'));
assertType('false', wp_http_validate_url(Faker::numericString()));

assertType("'url'|false", wp_http_validate_url('url'));

assertType('non-empty-string|false', wp_http_validate_url(Faker::string()));
assertType('non-empty-string|false', wp_http_validate_url(Faker::nonEmptyString()));
assertType('non-falsy-string|false', wp_http_validate_url(Faker::nonFalsyString()));
assertType('(lowercase-string&non-empty-string)|false', wp_http_validate_url(Faker::lowercaseString()));
assertType('(lowercase-string&non-falsy-string)|false', wp_http_validate_url(Faker::intersection(Faker::lowercaseString(), Faker::nonFalsyString())));
