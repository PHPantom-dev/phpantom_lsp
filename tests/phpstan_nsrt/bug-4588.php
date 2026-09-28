<?php

namespace Bug4588;

use function PHPStan\Testing\assertType;

class c {
	private $b;
	public function __construct(?b $b ) {
		$this->b = $b;
	}
	public function getB(): ?b {

		return $this->b;
	}
}

class b{
	public function callB():bool {return true;}
}

function (c $c): void {
	if ($c->getB()) {
		assertType(b::class, $c->getB());
	}
};
