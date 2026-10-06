<?php

namespace Bug5336;

use function PHPStan\Testing\assertType;

interface Stub
{
}

interface ProxyQueryInterface
{
}

/**
 * @phpstan-template T of ProxyQueryInterface
 */
class Pager
{
	/**
	 * @var T
	 */
	private $query;

	/**
	 * @phpstan-param T $query
	 */
	public function __construct(ProxyQueryInterface $query) {
		$this->query = $query;
	}
}

abstract class Test
{
	/**
	 * @template T of object
	 * @param class-string<T> $originalClassName
	 * @return T&Stub
	 */
	abstract public function createStub(string $originalClassName): Stub;

	public function sayHello(): void
	{
		$query = $this->createStub(ProxyQueryInterface::class);
		assertType('Bug5336\Pager<Bug5336\ProxyQueryInterface&Bug5336\Stub>', new Pager($query));
	}
}
