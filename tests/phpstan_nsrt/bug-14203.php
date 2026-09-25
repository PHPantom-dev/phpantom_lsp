<?php // lint >= 8.1

declare(strict_types = 1);

namespace Bug14203;

use function PHPStan\Testing\assertType;

/**
 * @template TKey of array-key
 * @template TValue
 */
class Collection
{
    /**
     * Create a new collection.
     *
     * @param  array<TKey, TValue>  $items
     */
    final public function __construct(protected $items = [])
    {
    }

    /**
	 * @template TMapValue
     *
     * @param  callable(TValue, TKey): TMapValue  $callback
     * @return static<TKey, TMapValue>
     */
    public function map(callable $callback)
    {
        $newItems = [];

		foreach ($this->items as $key => $value) {
			$newItems[$key] = $callback($value, $key);
		}

		return new static($newItems);
    }
}

class SpecificA {
    public function __construct(
		public readonly int $valueA,
		public readonly string $someSharedValue,
	) {}
}

class SpecificB {
    public function __construct(
		public readonly int $valueB,
		public readonly string $someSharedValue,
	) {}
}

class MyDTO {
    public function __construct(
		public readonly string $thatSharedValue,
	) {}
}

function works(): void {
    $myCollection = new Collection([new SpecificA(1, 'A'), new SpecificB(2, 'B')]);

	$result = $myCollection->map(static fn (SpecificA|SpecificB $specific): MyDTO => new MyDTO($specific->someSharedValue));
	assertType('Bug14203\Collection<int, Bug14203\MyDTO>', $result);
}

/**
 * @return Collection<int, SpecificA>
 */
function getA(): Collection {
	return new Collection([new SpecificA(1, 'A')]);
}

/**
 * @return Collection<int, SpecificB>
 */
function getB(): Collection {
	return new Collection([new SpecificB(2, 'B')]);
}

function breaks(): void {
    $myCollection = random_int(0, 1) === 0
		? getA()
		: getB();

	$result = $myCollection->map(static fn (SpecificA|SpecificB $specific): MyDTO => new MyDTO($specific->someSharedValue));
	assertType('Bug14203\Collection<int, Bug14203\MyDTO>', $result);
}
