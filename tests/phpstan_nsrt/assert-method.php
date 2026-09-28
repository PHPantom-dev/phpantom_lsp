<?php

namespace AssertMethod;

use function PHPStan\Testing\assertType;

interface Identity
{
	public function getId(): ?int;

	/**
	 * @phpstan-assert-if-true int $this->getId()
	 * @phpstan-assert-if-false null $this->getId()
	 */
	public function hasId(): bool;
}

function (Identity $identity) {
	assertType('int|null', $identity->getId());

	if ($identity->hasId()) {
		assertType('int', $identity->getId());
	} else {
		assertType('null', $identity->getId());
	}
};
