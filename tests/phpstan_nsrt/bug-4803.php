<?php

namespace Bug4803;

use function PHPStan\Testing\assertType;

/**
 * @template T of object
 */
interface Proxy {}

class Foo
{

	/**
	 * @template T of object
	 * @param Proxy<T>|T $proxyOrObject
	 * @return T
	 */
	public function doFoo($proxyOrObject)
	{
		assertType('Bug4803\Proxy<T of object (method Bug4803\Foo::doFoo(), argument)>|T of object (method Bug4803\Foo::doFoo(), argument)', $proxyOrObject);
	}

	/** @param Proxy<\stdClass> $proxy */
	public function doBar($proxy): void
	{
		assertType('stdClass', $this->doFoo($proxy));
	}

	/** @param \stdClass $std */
	public function doBaz($std): void
	{
		assertType('stdClass', $this->doFoo($std));
	}

	/** @param Proxy<\stdClass>|\stdClass $proxyOrStd */
	public function doLorem($proxyOrStd): void
	{
		assertType('stdClass', $this->doFoo($proxyOrStd));
	}

}

interface ProxyClassResolver
{
	/**
	 * @template T of object
	 * @param class-string<Proxy<T>>|class-string<T> $className
	 * @return class-string<T>
	 */
	public function resolveClassName(string $className): string;
}

final class Client
{
	/** @var ProxyClassResolver */
	private $proxyClassResolver;

	public function __construct(ProxyClassResolver $proxyClassResolver)
	{
		$this->proxyClassResolver = $proxyClassResolver;
	}

	/**
	 * @template T of object
	 * @param class-string<Proxy<T>>|class-string<T> $className
	 * @return class-string<T>
	 */
	public function getRealClass(string $className): string
	{
		assertType('class-string<Bug4803\Proxy<T of object (method Bug4803\Client::getRealClass(), argument)>>|class-string<T of object (method Bug4803\Client::getRealClass(), argument)>', $className);

		$result = $this->proxyClassResolver->resolveClassName($className);
		assertType('class-string<T of object (method Bug4803\Client::getRealClass(), argument)>', $result);

		return $result;
	}
}
