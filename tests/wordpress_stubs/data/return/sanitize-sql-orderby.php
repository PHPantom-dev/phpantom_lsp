<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function sanitize_sql_orderby;
use function PHPStan\Testing\assertType;

assertType('false', sanitize_sql_orderby(''));
assertType("'orderby'|false", sanitize_sql_orderby('orderby'));
assertType('non-falsy-string|false', sanitize_sql_orderby(Faker::string()));
assertType('(lowercase-string&non-falsy-string)|false', sanitize_sql_orderby(Faker::lowercaseString()));
assertType('non-falsy-string|false', sanitize_sql_orderby(Faker::nonEmptyString()));
assertType('non-falsy-string|false', sanitize_sql_orderby(Faker::nonFalsyString()));

assertType("'Orderby'|(lowercase-string&non-falsy-string)|false", sanitize_sql_orderby(Faker::union('Orderby', Faker::lowercaseString())));

assertType('(lowercase-string&non-falsy-string)|false', sanitize_sql_orderby(Faker::intersection(Faker::lowercaseString(), Faker::nonFalsyString())));
