<?php

namespace Bug4436;

use function PHPStan\Testing\assertType;

class Bar
{
}

class Foo
{
	/** @var \SplObjectStorage<Bar, string> */
	private $storage;

	public function __construct()
	{
		$this->storage = new \SplObjectStorage();
	}

	public function add(Bar $bar, string $value): void
	{
		$this->storage[$bar] = $value;
	}

	public function get(Bar $bar): string
	{
		assertType('string', $this->storage[$bar]);
		return $this->storage[$bar];
	}
}
