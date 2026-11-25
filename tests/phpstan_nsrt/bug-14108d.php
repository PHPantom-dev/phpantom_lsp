<?php // lint >= 8.0

namespace Bug14108d;

use function PHPStan\Testing\assertType;

class Foo
{
	public function __construct(private ?string $param)
	{
	}

	public function getParam(): ?string
	{
		return $this->param;
	}

	/**
	 * @phpstan-assert-if-true string $this->getParam()
	 */
	public function narrowGetParam(): bool
	{
	}
}

class Bar
{
	public function __construct(private ?int $param)
	{
	}

	public function getParam(): ?int
	{
		return $this->param;
	}

	/**
	 * @phpstan-assert int $this->getParam()
	 */
	public function narrowGetParam(): void
	{
	}
}

function test(Foo|Bar $fooOrBar): void
{
	assertType('int|string|null', $fooOrBar->getParam());

	$fooOrBar->narrowGetParam();

	assertType('int|string|null', $fooOrBar->getParam());

	if ($fooOrBar->narrowGetParam()) {
		assertType('int|string|null', $fooOrBar->getParam()); // could be 'int|string' // SKIP: an assertion one class of a union receiver makes narrows as if every class made it
	} else {
		assertType('int|string|null', $fooOrBar->getParam()); // SKIP: an assertion one class of a union receiver makes narrows as if every class made it
	}
	assertType('int|string|null', $fooOrBar->getParam());
}
