<?php declare(strict_types = 1);

namespace Bug9995;

use function PHPStan\Testing\assertType;

function test(?\DateTimeImmutable $a, ?\DateTimeImmutable $b): string
{
	if (null === ($a ??= ($b ? $b : null))) {
		throw new \LogicException('Either a or b MUST be set');
	}

	assertType('DateTimeImmutable' , $a);
	return $a->format('c');
}
