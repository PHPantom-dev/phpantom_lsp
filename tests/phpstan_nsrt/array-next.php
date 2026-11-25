<?php

namespace ArrayNext;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$array = [];
		assertType('false', next($array));
	}

	/**
	 * @param int[] $a
	 */
	public function doBar(array $a)
	{
		assertType('int|false', next($a));
	}

	/**
	 * @param non-empty-array<int, string> $a
	 */
	public function doBaz(array $a)
	{
		assertType('string|false', next($a));
	}

}

class Foo2
{

	public function doFoo()
	{
		$array = [];
		assertType('false', prev($array));
	}

	/**
	 * @param int[] $a
	 */
	public function doBar(array $a)
	{
		assertType('int|false', prev($a));
	}

	/**
	 * @param non-empty-array<int, string> $a
	 */
	public function doBaz(array $a)
	{
		assertType('string|false', prev($a));
	}

}

interface HttpClientPoolItem
{
	public function isDisabled(): bool;
}

final class RoundRobinClientPool
{
	/**
	 * @var HttpClientPoolItem[]
	 */
	protected $clientPool = [];

	protected function chooseHttpClient(): HttpClientPoolItem
	{
		$last = current($this->clientPool);
		assertType(HttpClientPoolItem::class . '|false', $last);

		do {
			$client = next($this->clientPool);
			assertType(HttpClientPoolItem::class . '|false', $client);

			if (false === $client) {
				$client = reset($this->clientPool);
				assertType(HttpClientPoolItem::class . '|false', $client);

				if (false === $client) {
					throw new \Exception();
				}

				assertType(HttpClientPoolItem::class, $client);
			}

			assertType(HttpClientPoolItem::class, $client);

			// Case when there is only one and the last one has been disabled
			if ($last === $client) {
				assertType(HttpClientPoolItem::class, $client);
				throw new \Exception();
			}
		} while ($client->isDisabled());

		return $client;
	}
}
