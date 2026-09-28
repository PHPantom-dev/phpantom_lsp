<?php

namespace Bug1216;

use AllowDynamicProperties;
use function PHPStan\Testing\assertType;

abstract class Foo
{
	/**
	 * @var int
	 */
	protected $foo;
}

trait Bar
{
	/**
	 * @var int
	 */
	protected $bar;

	protected $untypedBar;
}

/**
 * @property string $foo
 * @property string $bar
 * @property string $untypedBar
 */
#[AllowDynamicProperties]
class Baz extends Foo
{

	public function __construct()
	{
		assertType('string', $this->foo);
		assertType('string', $this->bar);
		assertType('string', $this->untypedBar);
	}

}

function (Baz $baz): void {
	assertType('string', $baz->foo);
	assertType('string', $baz->bar);
	assertType('string', $baz->untypedBar);
};
