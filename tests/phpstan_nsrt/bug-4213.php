<?php // lint >= 8.1

namespace Bug4213;

use function PHPStan\Testing\assertType;

abstract class BaseEnum
{
	/** @var string */
	private $value;

	final private function __construct(string $value)
	{
		$this->value = $value;
	}
	/**
	 * @return static
	 */
	public static function get(string $value): self {
		return new static($value);
	}
}

final class Enum extends BaseEnum
{
}

final class Entity {
	public function setEnums(Enum ...$enums): void {
	}
	/**
	 * @param Enum[] $enums
	 */
	public function setEnumsWithoutSplat(array $enums): void {
	}
}

function (): void {
	assertType('Bug4213\Enum', Enum::get('test'));
	assertType('array{Bug4213\\Enum}', array_map([Enum::class, 'get'], ['test'])); // SKIP: array_map() with a `[Foo::class, 'method']` callable does not read the method's return type
};


class Foo
{
	/**
	 * @return static
	 */
	public static function create() : Foo
	{
		return new static();
	}
}


class Bar extends Foo
{
}

function (): void {
	$cbFoo = [Foo::class, 'create'];
	$cbBar = [Bar::class, 'create'];
	assertType('Bug4213\Foo', $cbFoo());
	assertType('Bug4213\Bar', $cbBar());
};
