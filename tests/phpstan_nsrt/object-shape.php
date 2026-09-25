<?php

namespace ObjectShape;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param object{foo: self, bar: int, baz?: string} $o
	 */
	public function doFoo($o): void
	{
		assertType('Foo', $o->foo);
		assertType('int', $o->bar);
	}

	/**
	 * @param object{foo: self, bar: int, baz?: string} $o
	 */
	public function doFoo2(object $o): void
	{
		assertType('Foo', $o->foo);
		assertType('int', $o->bar);
	}

	/**
	 * @template T
	 * @param object{foo: int, bar: T} $o
	 * @return T
	 */
	public function generics(object $o)
	{

	}

	/**
	 * @return object{foo: self}
	 */
	public function returnObjectShape(): object
	{

	}

	public function testObjectShape()
	{
		$result = $this->returnObjectShape();
		assertType('Foo', $result->foo);
	}

}

class Bar
{

	/**
	 * @param object{name: string, age: int} $person
	 */
	public function doFoo(object $person): void
	{
		assertType('string', $person->name);
		assertType('int', $person->age);
	}

	/**
	 * @param object{id: int, tags: array<string>} $item
	 */
	public function nestedTypes(object $item): void
	{
		assertType('int', $item->id);
		assertType('array<string>', $item->tags);
	}

	/**
	 * @param object{inner: object{x: int, y: int}} $nested
	 */
	public function nestedShape(object $nested): void
	{
		assertType('int', $nested->inner->x);
		assertType('int', $nested->inner->y);
	}

	/**
	 * @return object{success: bool, message: string}
	 */
	public function returnShape(): object
	{
		return (object) ['success' => true, 'message' => 'ok'];
	}

	public function testReturnShape(): void
	{
		$result = $this->returnShape();
		assertType('bool', $result->success);
		assertType('string', $result->message);
	}

}