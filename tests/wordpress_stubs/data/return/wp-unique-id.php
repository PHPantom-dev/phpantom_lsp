<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_unique_id;
use function wp_unique_prefixed_id;
use function PHPStan\Testing\assertType;

// wp_unique_id

assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_id());
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_id(''));
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_id('1'));
assertType('non-falsy-string&numeric-string', wp_unique_id(Faker::numericString())); // numeric-string may contain uppercase letters: is_numeric('1E10');

assertType('non-falsy-string', wp_unique_id('String'));
assertType('non-falsy-string', wp_unique_id(Faker::string()));

assertType('lowercase-string&non-falsy-string', wp_unique_id('string'));
assertType('lowercase-string&non-falsy-string', wp_unique_id(Faker::lowercaseString()));
assertType('non-falsy-string', wp_unique_id(Faker::uppercaseString()));

// wp_unique_id_from_values

assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values([]));
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(['key' => 'value']));
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array()));
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array(), ''));
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array(), 'prefix'));
assertType('non-falsy-string', wp_unique_id_from_values(Faker::array(), 'Prefix'));
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array(), Faker::lowercaseString()));
assertType('non-falsy-string', wp_unique_id_from_values(Faker::array(), Faker::string()));

// wp_unique_prefixed_id

assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_prefixed_id());
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_prefixed_id(''));
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_prefixed_id('1'));
assertType('non-falsy-string&numeric-string', wp_unique_prefixed_id(Faker::numericString())); // numeric-string may contain uppercase letters: is_numeric('1E10');

assertType('non-falsy-string', wp_unique_prefixed_id('String'));
assertType('non-falsy-string', wp_unique_prefixed_id(Faker::string()));

assertType('lowercase-string&non-falsy-string', wp_unique_prefixed_id('string'));
assertType('lowercase-string&non-falsy-string', wp_unique_prefixed_id(Faker::lowercaseString()));
assertType('non-falsy-string', wp_unique_prefixed_id(Faker::uppercaseString()));
