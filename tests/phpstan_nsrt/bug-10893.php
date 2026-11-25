<?php declare(strict_types = 1);

namespace Bug10893;

use function PHPStan\Testing\assertType;

/**
 * @param non-falsy-string&numeric-string $str
 */
function hasMicroseconds(\DateTimeInterface $value, string $str): bool
{
	assertType('non-falsy-string&numeric-string', $str);
	assertType('int', (int)$str);
	assertType('bool', (int)$str !== 0);

	assertType('int', (int)$value->format('u'));
	assertType('bool', (int)$value->format('u') !== 0);

	assertType('int', (int)$value->format('v'));
	assertType('bool', (int)$value->format('v') !== 0);

	assertType('float', $value->format('u') * 1e-6);
	assertType('float', $value->format('v') * 1e-3);

	return (int) $value->format('u') !== 0;
}
