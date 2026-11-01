<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_term_by;
use function PHPStan\Testing\assertType;

assertType('WP_Term|false', get_term_by('term_id', Faker::int(), '', OBJECT));
assertType('WP_Term|false', get_term_by('slug', 'test'));
assertType('array<string, int|string>|false', get_term_by('term_id', Faker::int(), '', ARRAY_A));
assertType('list<int|string>|false', get_term_by('term_id', Faker::int(), '', ARRAY_N));
