<?php

namespace AssertThis;

use RuntimeException;

use function PHPStan\Testing\assertType;

/**
 * @template TOk
 * @template TErr
 */
interface Result {
	/**
	 * @phpstan-assert-if-true Ok<TOk> $this
	 * @phpstan-assert-if-false Err<TErr> $this
	 */
	public function isOk(): bool;

	/**
	 * @return TOk|never
	 */
	public function unwrap();
}

/**
 * @template TOk
 * @template-implements Result<TOk, never>
 */
class Ok implements Result {
	public function __construct(private $value) {
	}

	/**
	 * @return true
	 */
	public function isOk(): bool {
		return true;
	}

	/**
	 * @return TOk
	 */
	public function unwrap() {
		return $this->value;
	}
}

/**
 * @template TErr
 * @template-implements Result<never, TErr>
 */
class Err implements Result {
	public function __construct(private $value) {
	}

	/**
	 * @return false
	 */
	public function isOk(): bool {
		return false;
	}
	
	/**
	 * @return never
	 */
	public function unwrap() {
		throw new RuntimeException('Tried to unwrap() an Err value');
	}
}

function () {
	/** @var Result<int, string> $result */
	$result = new Ok(123);
	assertType('AssertThis\\Result<int, string>', $result);
	
	if ($result->isOk()) {
		assertType('AssertThis\\Ok<int>', $result);
		assertType('int', $result->unwrap());
	} else { 
		assertType('AssertThis\\Err<string>', $result);
		assertType('never', $result->unwrap());
	}
};