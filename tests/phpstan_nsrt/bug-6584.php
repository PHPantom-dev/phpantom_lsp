<?php

namespace Bug6584;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(int $int, ?int $intOrNull)
	{
		assertType('int', $this->same($int));
		assertType('int', $this->sameWithDefault($int));

		assertType('int|null', $this->same($intOrNull));
		assertType('int|null', $this->sameWithDefault($intOrNull));

		assertType('null', $this->same(null));
		assertType('null', $this->sameWithDefault(null));
		assertType('null', $this->sameWithDefault());
	}


	/**
	 * @template T
	 * @param T $t
	 * @return T
	 */
	function same($t) {
		assertType('T (method Bug6584\Foo::same(), argument)', $t);
		return $t;
	}

	/**
	 * @template T
	 * @param T $t
	 * @return T
	 */
	function sameWithDefault($t = null) {
		assertType('T (method Bug6584\Foo::sameWithDefault(), argument)', $t);
		return $t;
	}

}
