<?php

namespace Bug6473;

use function PHPStan\Testing\assertType;

class Point {
	public bool $visited = false;

	/**
	 * @return Point[]
	 */
	public function getNeighbours(): array {
		return [ new Point ];
	}

	public function doFoo()
	{
		$seen = [];

		foreach([new Point, new Point] as $p ) {

			$p->visited = true;
			assertType('true', $p->visited);
			$seen = [
				... $seen,
				... array_filter( $p->getNeighbours(), static fn (Point $p) => !$p->visited )
			];
			assertType('true', $p->visited);
		}
	}

	public function doFoo2()
	{
		$seen = [];

		foreach([new Point, new Point] as $p ) {

			$p->visited = true;
			assertType('true', $p->visited);
			$seen = [
				... $seen,
				... array_filter( $p->getNeighbours(), static fn (Point $p2) => !$p2->visited )
			];
			assertType('true', $p->visited);
		}
	}
}
