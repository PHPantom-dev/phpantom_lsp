<?php

namespace InArrayTypeSpecifyingExtension;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param string $s
	 * @param string $r
	 * @param $mixed
	 * @param string[] $strings
	 */
	public function doFoo(
		string $s,
		string $r,
		$mixed,
		array $strings
	)
	{
		if (!in_array($s, ['foo', 'bar'], true)) {
			return;
		}

		if (!in_array($mixed, $strings, true)) {
			return;
		}

		if (in_array($r, $strings, true)) {
			return;
		}

		$fooOrBarOrBaz = 'foo';
		if (rand(0, 1) === 1) {
			$fooOrBarOrBaz = 'bar';
		} elseif (rand(0, 1) === 1) {
			$fooOrBarOrBaz = 'baz';
		}

		if (in_array($fooOrBarOrBaz, ['bar', 'baz'], true)) {
			return;
		}

		assertType('\'bar\'|\'foo\'', $s);
		assertType('string', $mixed);
		assertType('string', $r);
		assertType('\'foo\'', $fooOrBarOrBaz);
	}

	/** @param array<string> $strings */
	public function doBar(int $i, array $strings): void
	{
		assertType('bool', in_array($i, $strings));
		assertType('bool', in_array($i, $strings, false));
	}

}
