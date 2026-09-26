<?php

namespace Bug13537;

use stdClass;
use function PHPStan\Testing\assertType;

class Foo
{
	/**
	 * @var array<non-empty-string, mixed>
	 */
	protected array $test = [];
}

/**
 * @property stdClass $test
 */
class Bar extends Foo
{

}

function (): void {
	$bar = new Bar();
	$bob = $bar->test->bob;
};

class BaseModel
{
	/**
	 * @var array<non-empty-string, mixed>
	 */
	protected array $attributes = [];

	public function __get(string $key): mixed {
		return $this->attributes[$key] ?? null;
	}

	public function __isset(string $key): bool {
		return isset($this->attributes[$key]);
	}
}

/**
 * @property stdClass $attributes
 * @property bool $other
 */
class Bar2 extends BaseModel
{
	public function __constructor(): void {
		$this->attributes = [
			'attributes' => (object) array('foo' => 'bar'),
			'other' => true
		];
	}
}

function (): void {
	$bar = new Bar2();
	echo $bar->attributes->foo;

	assertType(stdClass::class, $bar->attributes);
};

class Test
{
	public function doFoo(): void
	{
		$bar = new Bar2();
		echo $bar->attributes->foo;

		assertType(stdClass::class, $bar->attributes);
	}
}
