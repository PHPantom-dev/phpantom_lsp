<?php declare(strict_types = 1);

namespace Bug7115;

use function array_push;
use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @return array<array{a:int,b:string}>
	 */
	public function getThings(): array { return []; }

	public function doFoo(): void
	{
		$a = $this->getThings();
		$b = [];
		$c = [];
		$d = [];

		array_push($b, ...$a);

		foreach ($a as $thing) {
			$c[] = $thing;
			array_push($d, $thing);
		}

		assertType('list<array{a: int, b: string}>', $b);
		assertType('list<array{a: int, b: string}>', $c);
		assertType('list<array{a: int, b: string}>', $d);
	}

}

