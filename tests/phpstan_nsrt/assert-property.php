<?php

namespace AssertProperty;

use function PHPStan\Testing\assertType;

class Identity
{
	/** @var int|null */
	public $id;

	/**
	 * @phpstan-assert-if-true int $this->id
	 * @phpstan-assert-if-false null $this->id
	 */
	public function hasId(): bool
	{
		return $this->id !== null;
	}
}

function (Identity $identity) {
	assertType('int|null', $identity->id);

	if ($identity->hasId()) {
		assertType('int', $identity->id);
	} else {
		assertType('null', $identity->id);
	}
};
