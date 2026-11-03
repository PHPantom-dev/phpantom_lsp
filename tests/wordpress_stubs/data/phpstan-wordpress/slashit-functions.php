<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function backslashit;
use function trailingslashit;
use function untrailingslashit;
use function PHPStan\Testing\assertType;

/*
 * trailingslashit()
 */

assertType("'/'", trailingslashit(''));
assertType("'0/'", trailingslashit('0'));
assertType("'foo/'", trailingslashit('foo'));
assertType("'foo/'", trailingslashit('foo/'));
assertType("'foo/'", trailingslashit('foo//'));
assertType("'foo/'", trailingslashit('foo\\'));

/** @var non-empty-string $nonEmptyString */
assertType('non-falsy-string', trailingslashit($nonEmptyString));

/** @var non-falsy-string $nonFalsyString */
assertType('non-falsy-string', trailingslashit($nonFalsyString));

/** @var lowercase-string&non-empty-string $lowercaseNonEmptyString */
assertType('lowercase-string&non-falsy-string', trailingslashit($lowercaseNonEmptyString));

/*
 * untrailingslashit()
 */

assertType("''", untrailingslashit(''));
assertType("'0'", untrailingslashit('0'));
assertType("'foo'", untrailingslashit('foo'));
assertType("'foo'", untrailingslashit('foo/'));
assertType("'foo'", untrailingslashit('foo//'));
assertType("'foo'", untrailingslashit('foo\\'));

/** @var non-empty-string $nonEmptyString */
assertType('string', untrailingslashit($nonEmptyString));

/** @var non-falsy-string $nonFalsyString */
assertType('string', untrailingslashit($nonFalsyString));

/** @var lowercase-string&non-empty-string $lowercaseNonEmptyString */
assertType('lowercase-string', untrailingslashit($lowercaseNonEmptyString));

/*
 * backslashit()
 */

assertType("''", backslashit(''));
// Upstream expects `literal-string&lowercase-string&non-falsy-string&uppercase-string`
// and `literal-string&lowercase-string&non-falsy-string`, the refinements of
// the literal WordPress returns.
assertType("'\\\\\\\\0'", backslashit('0'));
assertType("'\\\\f\\\\o\\\\o'", backslashit('foo'));

/** @var non-empty-string $nonEmptyString */
assertType('non-falsy-string', backslashit($nonEmptyString));

/** @var non-falsy-string $nonFalsyString */
assertType('non-falsy-string', backslashit($nonFalsyString));

/** @var lowercase-string&non-empty-string $lowercaseNonEmptyString */
assertType('lowercase-string&non-falsy-string', backslashit($lowercaseNonEmptyString));

/** @var numeric-string $numericString */
assertType('non-falsy-string', backslashit($numericString));

/** @var string $aString */
assertType('string', backslashit($aString));
