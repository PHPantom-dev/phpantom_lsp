<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function normalize_whitespace;
use function PHPStan\Testing\assertType;

assertType("''", normalize_whitespace(''));
assertType("''", normalize_whitespace(' '));
assertType("'0'", normalize_whitespace(' 0 '));

// Upstream expects `literal-string&lowercase-string&non-falsy-string` and
// `literal-string&non-falsy-string`, the refinements of the literal
// WordPress returns.
assertType("'foo'", normalize_whitespace(' foo '));
assertType("'Foo'", normalize_whitespace(' Foo '));

/** @var non-empty-string $nonEmptyString */
assertType('string', normalize_whitespace($nonEmptyString));

/** @var non-falsy-string $nonFalsyString */
// Upstream expects `non-empty-string`, but a string of spaces is non-falsy
// and normalizes to ''.
assertType('string', normalize_whitespace($nonFalsyString));

/** @var lowercase-string&non-falsy-string $nonFalsyLowercaseString */
assertType('lowercase-string', normalize_whitespace($nonFalsyLowercaseString));

/** @var literal-string $literalString */
assertType('literal-string', normalize_whitespace($literalString));
