<?php

namespace Bug11679;

use function PHPStan\Testing\assertType;

class WorkingExample
{
	/** @var array{foo?: bool} */
	private array $arr = [];

	public function sayHello(): bool
	{
		assertType('array{foo?: bool}', $this->arr);
		if (!isset($this->arr['foo'])) {
			$this->arr['foo'] = true;
			assertType('array{foo: bool}', $this->arr); // SKIP: writing into a property's array offset does not narrow the property
		}
		assertType('array{foo: bool}', $this->arr); // SKIP: writing into a property's array offset does not narrow the property
		return $this->arr['foo']; // PHPStan realizes optional 'foo' is set
	}
}

class NonworkingExample
{
	/** @var array<int, array{foo?: bool}> */
	private array $arr = [];

	public function sayHello(int $index): bool
	{
		assertType('array<int, array{foo?: bool}>', $this->arr);
		if (!isset($this->arr[$index]['foo'])) {
			$this->arr[$index]['foo'] = true;
			assertType('bool', $this->arr[$index]['foo']); // SKIP: writing into a property's array offset does not narrow the property
		}
		return $this->arr[$index]['foo']; // PHPStan does not realize 'foo' is set
	}
}
