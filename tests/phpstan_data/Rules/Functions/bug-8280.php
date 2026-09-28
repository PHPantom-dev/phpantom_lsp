<?php declare(strict_types = 1);

namespace Bug8280;

use function PHPStan\Testing\assertType;

/**
 * @param list<string> $var
 */
function foo($var): void {}

/** @var string|list<string>|null $var */
if (null !== $var) {
	assertType('list<string>', (array) $var);
	foo((array) $var); // should work the same as line below
	assertType('list<string>', !is_array($var) ? [$var] : $var);
	foo(!is_array($var) ? [$var] : $var);
}
