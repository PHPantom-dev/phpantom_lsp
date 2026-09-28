<?php

namespace UnableToResolveCallbackParameterType;

use function PHPStan\Testing\assertType;

/**
 * @template CallbackInput of mixed
 */
final class Callback
{
	/**
	 * @var callable(CallbackInput $input): bool
	 */
	private $callback;

	/** @param callable(CallbackInput $input): bool $callback */
	public function __construct(callable $callback)
	{
		$this->callback = $callback;
	}

	/**
	 * Returns a string representation of the constraint.
	 */
	public function toString(): string
	{
		return 'is accepted by specified callback';
	}

	/**
	 * Evaluates the constraint for parameter $value. Returns true if the
	 * constraint is met, false otherwise.
	 *
	 * @param CallbackInput $other
	 */
	protected function matches($other): bool
	{
		return ($this->callback)($other);
	}
}


class Foo
{

	/**
	 * @template CallbackInput of mixed
	 * @param callable(CallbackInput $callback): bool $callback
	 * @return Callback<CallbackInput>
	 */
	public function callback(callable $callback): Callback
	{
		return new Callback($callback);
	}

	public function test(): void
	{
		$cb = $this->callback(function (int $i): bool {
			return true;
		});
		assertType(Callback::class . '<int>', $cb);
	}

}
