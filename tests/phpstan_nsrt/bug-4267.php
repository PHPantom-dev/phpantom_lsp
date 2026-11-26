<?php

namespace Bug4267;

use function PHPStan\Testing\assertType;

/**
 * @implements \IteratorAggregate<int, static>
 */
class Model1 implements \IteratorAggregate
{

	#[\ReturnTypeWillChange]
	public function getIterator(): iterable
	{
		throw new \Exception('not implemented');
	}
}

class HelloWorld1 extends Model1
{
	/** @var int */
	public $x = 5;
}

function (): void {
	foreach (new HelloWorld1() as $h) {
		assertType(HelloWorld1::class, $h);
	}
};

class Model2 implements \IteratorAggregate
{
	/**
	 * @return iterable<static>
	 */
	#[\ReturnTypeWillChange]
	public function getIterator(): iterable
	{
		throw new \Exception('not implemented');
	}
}

class HelloWorld2 extends Model2
{
	/** @var int */
	public $x = 5;
}

function (): void {
	foreach (new HelloWorld2() as $h) {
		assertType(HelloWorld2::class, $h);
	}
};
