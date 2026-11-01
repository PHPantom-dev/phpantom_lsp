<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_term_by;
use function PHPStan\Testing\assertType;

assertType('WP_Term|false', get_term_by('term_id', Faker::int(), '', OBJECT)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('WP_Term|false', get_term_by('slug', 'test')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('array<string, int|string>|false', get_term_by('term_id', Faker::int(), '', ARRAY_A)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('list<int|string>|false', get_term_by('term_id', Faker::int(), '', ARRAY_N)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
