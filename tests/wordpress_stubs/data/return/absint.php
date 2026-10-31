<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function absint;
use function PHPStan\Testing\assertType;

assertType('int<0, max>', absint(Faker::int()));

// Returns input for non-negative integers
assertType('0', absint(0)); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('1', absint(1)); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('10', absint(10)); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('int<1, max>', absint(Faker::positiveInt())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('int<0, max>', absint(Faker::nonNegativeInt()));

// Returns 0 for "empty" input
assertType('0', absint(null)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('0', absint(false)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('0', absint(0.0)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('0', absint(''));
assertType('0', absint([])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
// and non-numeric string
assertType('0', absint('nonNumericString'));
assertType('0', absint(' '));

// Returns 1 for true and non-empty array
assertType('1', absint(true));
assertType('1', absint(['key' => 'value']));

// Returns 0 or 1 for booleans
assertType('0|1', absint(Faker::bool())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

// Returns positive integer for strictly negative integer input
assertType('int<1, max>', absint(-1)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('int<1, max>', absint(-10)); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

// Returns non-negative integer for floats, numeric strings, resources
assertType('int<0, max>', absint(1.0));
assertType('int<0, max>', absint(Faker::float()));
assertType('int<0, max>', absint('-10')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('int<0, max>', absint(Faker::numericString()));
assertType('int<0, max>', absint(Faker::resource())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
// and any other type that is not a subtype of `$maybeint`
assertType('int<0, max>', absint(Faker::stdClass())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
