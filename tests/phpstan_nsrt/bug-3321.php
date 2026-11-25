<?php

namespace Bug3321;

use function PHPStan\Testing\assertType;

/** @template T */
interface Container {
	/** @return T */
	public function get();
}


class Foo
{

	/**
	 * @template T
	 * @param array<Container<T>> $containers
	 * @return T
	 */
	function unwrap(array $containers) {
		return array_map(
			function ($container) {
				return $container->get();
			},
			$containers
		)[0];
	}

	/**
	 * @param array<Container<int>> $typed_containers
	 */
	function takesDifferentTypes(array $typed_containers): void {
		assertType('int', $this->unwrap($typed_containers));
	}

}

/**
 * @template TFacade of Facade
 */
interface Facadable
{
}

/**
 * @implements Facadable<AFacade>
 */
class A implements Facadable {}

/**
 * @implements Facadable<BFacade>
 */
class B implements Facadable {}

abstract class Facade {}
class AFacade extends Facade {}
class BFacade extends Facade {}

class FacadeFactory {
	/**
	 * @template TFacade of Facade
	 * @param class-string<Facadable<TFacade>> $class
	 * @return TFacade
	 */
	public function create(string $class): Facade
	{
		// Returns facade for class
	}
}


function (FacadeFactory $f): void {
	$facadeA = $f->create(A::class);
	assertType(AFacade::class, $facadeA);
};
