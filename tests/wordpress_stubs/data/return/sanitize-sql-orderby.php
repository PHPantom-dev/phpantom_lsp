<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function sanitize_sql_orderby;
use function PHPStan\Testing\assertType;

assertType('false', sanitize_sql_orderby(''));
assertType("'orderby'|false", sanitize_sql_orderby('orderby')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('non-falsy-string|false', sanitize_sql_orderby(Faker::string())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('(lowercase-string&non-falsy-string)|false', sanitize_sql_orderby(Faker::lowercaseString())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('non-falsy-string|false', sanitize_sql_orderby(Faker::nonEmptyString())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('non-falsy-string|false', sanitize_sql_orderby(Faker::nonFalsyString()));

assertType("'Orderby'|(lowercase-string&non-falsy-string)|false", sanitize_sql_orderby(Faker::union('Orderby', Faker::lowercaseString()))); // SKIP: a conditional over a method template ignores its bound and does not narrow it

assertType('(lowercase-string&non-falsy-string)|false', sanitize_sql_orderby(Faker::intersection(Faker::lowercaseString(), Faker::nonFalsyString()))); // SKIP: a conditional over a method template ignores its bound and does not narrow it
