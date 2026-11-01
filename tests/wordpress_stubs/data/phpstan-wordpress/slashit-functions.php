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

assertType("'/'", trailingslashit('')); // SKIP: a call on literal arguments is not evaluated
assertType("'0/'", trailingslashit('0')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo/'", trailingslashit('foo')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo/'", trailingslashit('foo/')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo/'", trailingslashit('foo//')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo/'", trailingslashit('foo\\')); // SKIP: a call on literal arguments is not evaluated

/** @var non-empty-string $nonEmptyString */
assertType('non-falsy-string', trailingslashit($nonEmptyString));

/** @var non-falsy-string $nonFalsyString */
assertType('non-falsy-string', trailingslashit($nonFalsyString));

/** @var lowercase-string&non-empty-string $lowercaseNonEmptyString */
assertType('lowercase-string&non-falsy-string', trailingslashit($lowercaseNonEmptyString));

/*
 * untrailingslashit()
 */

assertType("''", untrailingslashit('')); // SKIP: a call on literal arguments is not evaluated
assertType("'0'", untrailingslashit('0')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo'", untrailingslashit('foo')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo'", untrailingslashit('foo/')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo'", untrailingslashit('foo//')); // SKIP: a call on literal arguments is not evaluated
assertType("'foo'", untrailingslashit('foo\\')); // SKIP: a call on literal arguments is not evaluated

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
assertType('literal-string&lowercase-string&non-falsy-string&uppercase-string', backslashit('0')); // SKIP: a call on literal arguments is not evaluated
assertType('literal-string&lowercase-string&non-falsy-string', backslashit('foo')); // SKIP: a call on literal arguments is not evaluated

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
