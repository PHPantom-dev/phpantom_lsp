<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_term;
use function PHPStan\Testing\assertType;

assertType('WP_Error|WP_Term|null', get_term(2, '', OBJECT));
assertType('WP_Error|WP_Term|null', get_term(2, 'category', OBJECT));
assertType('WP_Error|WP_Term|null', get_term(2));

assertType('array<string, int|string>|WP_Error|null', get_term(2, '', ARRAY_A));
assertType('array<string, int|string>|WP_Error|null', get_term(2, 'category', ARRAY_A));
assertType('list<int|string>|WP_Error|null', get_term(2, '', ARRAY_N));
assertType('list<int|string>|WP_Error|null', get_term(2, 'category', ARRAY_N));
