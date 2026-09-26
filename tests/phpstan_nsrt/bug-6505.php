<?php // lint >= 8.0

namespace Bug6505;

use function PHPStan\Testing\assertType;

/** @template T */
interface Type
{
	/**
	 * @param T $val
	 * @return T
	 */
	public function validate($val);
}

/**
 * @template T
 * @implements Type<class-string<T>>
 */
final class ClassStringType implements Type
{
	/** @param class-string<T> $classString */
	public function __construct(public string $classString)
	{
	}

	public function validate($val) {
		return $val;
	}
}

/**
 * @implements Type<class-string<\stdClass>>
 */
final class StdClassType implements Type
{
	public function validate($val) {
		return $val;
	}
}


/**
 * @template T
 * @implements Type<T[]>
 */
final class TypeCollection implements Type
{
	/** @param Type<T> $type */
	public function __construct(public Type $type)
	{
	}
	public function validate($val) {
		return $val;
	}
}

class Foo
{

	public function doFoo()
	{
		$c = new TypeCollection(new ClassStringType(\stdClass::class));
		assertType('array<class-string<stdClass>>', $c->validate([\stdClass::class]));
		$c2 = new TypeCollection(new StdClassType());
		assertType('array<class-string<stdClass>>', $c2->validate([\stdClass::class]));
	}

	/**
	 * @template T
	 * @param T $t
	 * @return T
	 */
	function unbounded($t) {
		return $t;
	}

	/**
	 * @template T of string
	 * @param T $t
	 * @return T
	 */
	function bounded1($t) {
		return $t;
	}

	/**
	 * @template T of object|class-string
	 * @param T $t
	 * @return T
	 */
	function bounded2($t) {
		return $t;
	}

	/** @param class-string<\stdClass> $p */
	function test($p): void {
		assertType('class-string<stdClass>', $this->unbounded($p));
		assertType('class-string<stdClass>', $this->bounded1($p));
		assertType('class-string<stdClass>', $this->bounded2($p));
	}

}

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection
{
	/**
	 * @var array<TKey, TValue>
	 */
	protected array $items;

	/**
	 * Create a new collection.
	 *
	 * @param array<TKey, TValue>|null $items
	 * @return void
	 */
	public function __construct(?array $items = [])
	{
		$this->items = $items ?? [];
	}
}

class Example
{
	/** @var array<string, list<class-string>> */
	private array $factories = [];

	public function getFactories(): void
	{
		assertType('Bug6505\Collection<string, list<class-string>>', new Collection($this->factories));
	}
}

