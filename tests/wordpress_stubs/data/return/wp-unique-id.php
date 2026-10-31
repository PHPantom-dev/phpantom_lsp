<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_unique_id;
use function wp_unique_prefixed_id;
use function PHPStan\Testing\assertType;

// wp_unique_id

assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_id()); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_id('')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_id('1')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('non-falsy-string&numeric-string', wp_unique_id(Faker::numericString())); // numeric-string may contain uppercase letters: is_numeric('1E10'); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

assertType('non-falsy-string', wp_unique_id('String')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('non-falsy-string', wp_unique_id(Faker::string())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

assertType('lowercase-string&non-falsy-string', wp_unique_id('string')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('lowercase-string&non-falsy-string', wp_unique_id(Faker::lowercaseString())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('non-falsy-string', wp_unique_id(Faker::uppercaseString())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

// wp_unique_id_from_values

assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values([])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(['key' => 'value'])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array(), '')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array(), 'prefix')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('non-falsy-string', wp_unique_id_from_values(Faker::array(), 'Prefix'));
assertType('lowercase-string&non-falsy-string', wp_unique_id_from_values(Faker::array(), Faker::lowercaseString())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('non-falsy-string', wp_unique_id_from_values(Faker::array(), Faker::string()));

// wp_unique_prefixed_id

assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_prefixed_id()); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_prefixed_id('')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('lowercase-string&non-falsy-string&numeric-string', wp_unique_prefixed_id('1')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('non-falsy-string&numeric-string', wp_unique_prefixed_id(Faker::numericString())); // numeric-string may contain uppercase letters: is_numeric('1E10'); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

assertType('non-falsy-string', wp_unique_prefixed_id('String')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('non-falsy-string', wp_unique_prefixed_id(Faker::string())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

assertType('lowercase-string&non-falsy-string', wp_unique_prefixed_id('string')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('lowercase-string&non-falsy-string', wp_unique_prefixed_id(Faker::lowercaseString())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('non-falsy-string', wp_unique_prefixed_id(Faker::uppercaseString())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
