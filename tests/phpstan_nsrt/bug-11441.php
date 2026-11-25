<?php // lint >= 8.0

namespace Bug11441;

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
	 * @phpstan-assert !null $this->getParam()
	 */
	public function checkNotNull(): void
	{
		if ($this->getParam() === null) {
			throw new \Exception();
		}
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
	 * @phpstan-assert !null $this->getParam()
	 */
	public function checkNotNull(): void
	{
		if ($this->getParam() === null) {
			throw new \Exception();
		}
	}
}

function test(Foo|Bar $fooOrBar): void
{
	assertType('int|string|null', $fooOrBar->getParam());

	$fooOrBar->checkNotNull();

	assertType('int|string', $fooOrBar->getParam());
}
