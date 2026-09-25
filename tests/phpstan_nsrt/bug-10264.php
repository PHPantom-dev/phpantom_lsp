<?php

namespace Bug10264;

use function PHPStan\Testing\assertType;
use stdClass;

class A
{
	function doFoo() {
		/** @var list<A> $list */
		$list = [];

		assertType('list<Bug10264\A>', $list);

		assert((count($list) <= 1) === true);
		assertType('list<Bug10264\A>', $list);
	}

	function doFoo2() {
		/** @var list<A> $list */
		$list = [];

		assertType('list<Bug10264\A>', $list);

		assert((count($list, COUNT_NORMAL) <= 1) === true);
		assertType('list<Bug10264\A>', $list);
	}

	/** @param list<int> $c */
	public function sayHello(array $c): void
	{
		assertType('list<int>', $c);
		if (count($c) > 0) {
			$c = array_map(fn() => new stdClass(), $c);
		} else {
			assertType('array{}', $c);
		}

		assertType('list<stdClass>', $c); // SKIP: joining array{} with a non-empty list loses the element type
	}

	function doBar() {
		/** @var list<A> $list */
		$list = [];

		assertType('list<Bug10264\A>', $list);

		assert((count($list, COUNT_RECURSIVE) <= 1) === true);
		assertType('list<Bug10264\A>', $list);
	}

	function doIf():void {
		/** @var list<A> $list */
		$list = [];

		assertType('list<Bug10264\A>', $list);

		if( count($list, COUNT_RECURSIVE) >= 1) {
			assertType('non-empty-list<Bug10264\A>', $list);
		} else {
			assertType('array{}', $list);
		}
	}

	function countModeInt(int $i):void {
		/** @var list<A> $list */
		$list = [];

		assertType('list<Bug10264\A>', $list);

		if( count($list, $i) >= 1) {
			assertType('non-empty-list<Bug10264\A>', $list);
		} else {
			assertType('array{}', $list);
		}
	}

}
