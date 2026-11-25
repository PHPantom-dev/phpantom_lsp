<?php declare(strict_types = 1);

namespace Bug11642;

use function PHPStan\Testing\assertType;

class Repository
{
	/**
	 * @psalm-param array<string|int, mixed> $criteria
	 *
	 * @return object[] The objects.
	 * @psalm-return list<\stdClass>
	 */
	function findBy(array $criteria): array
	{
		return [new \stdClass, new \stdCLass, new \stdClass, new \stdClass];
	}
}

class Payload {
	/** @var non-empty-list<string> */
	public array $ids = ['one', 'two'];
}

function doFoo() {
	$payload = new Payload();

	$fetcher = new Repository();
	$entries = $fetcher->findBy($payload->ids);
	assertType('list<stdClass>', $entries);
	assertType('int<0, max>', count($entries));
	if (count($entries) !== count($payload->ids)) {
		exit();
	}

	if (count($entries) > 3) {
		throw new \RuntimeException();
	}

}
