<?php

namespace IteratorToArray;

use stdClass;
use Traversable;
use function iterator_to_array;
use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @param Traversable<string, int> $foo
	 */
	public function testDefaultBehavior(Traversable $foo)
	{
		assertType('array<string, int>', iterator_to_array($foo));
	}

	/**
	 * @param Traversable<string, string> $foo
	 */
	public function testExplicitlyPreserveKeys(Traversable $foo)
	{
		assertType('array<string, string>', iterator_to_array($foo, true));
	}

	/**
	 * @param Traversable<string, string> $foo
	 */
	public function testNotPreservingKeys(Traversable $foo)
	{
		assertType('list<string>', iterator_to_array($foo, false));
	}

	public function testBehaviorOnGenerators(): void
	{
		$generator1 = static function (): iterable {
			yield 0 => 1;
			yield true => 2;
			yield 2 => 3;
			yield null => 4;
		};
		$generator2 = static function (): iterable {
			yield 0 => 1;
			yield 'a' => 2;
			yield null => 3;
			yield true => 4;
		};

		assertType('array<0|1|2|\'\', 1|2|3|4>', iterator_to_array($generator1())); // SKIP: a closure declared `iterable` is not typed by the Generator its body yields
		assertType('list<1|2|3|4>', iterator_to_array($generator1(), false)); // SKIP: a closure declared `iterable` is not typed by the Generator its body yields
		assertType('array<0|1|\'\'|\'a\', 1|2|3|4>', iterator_to_array($generator2())); // SKIP: a closure declared `iterable` is not typed by the Generator its body yields
		assertType('list<1|2|3|4>', iterator_to_array($generator2(), false)); // SKIP: a closure declared `iterable` is not typed by the Generator its body yields
	}

	public function testOnGeneratorsWithIllegalKeysForArray(): void
	{
		$illegalGenerator = static function (): iterable {
			yield 'a' => 'b';
			yield new stdClass => 'c';
		};

		assertType('*NEVER*', iterator_to_array($illegalGenerator())); // SKIP: a closure declared `iterable` is not typed by the Generator its body yields
		assertType('list<\'b\'|\'c\'>', iterator_to_array($illegalGenerator(), false)); // SKIP: a closure declared `iterable` is not typed by the Generator its body yields
	}
}
