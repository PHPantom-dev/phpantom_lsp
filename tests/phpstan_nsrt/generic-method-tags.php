<?php // lint >= 8.0

namespace GenericMethodTags;

use function PHPStan\Testing\assertType;

/**
 * @method TVal doThing<TVal of mixed>(TVal $param)
 * @method TVal doAnotherThing(int $param)
 */
class Test
{
	public function __call(): mixed
	{
	}
}

function test(int $int, string $string): void
{
	$test = new Test();

	assertType('int', $test->doThing($int));
	assertType('string', $test->doThing($string));
	assertType(TVal::class, $test->doAnotherThing($int));
}
