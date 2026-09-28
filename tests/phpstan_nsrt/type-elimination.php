<?php

namespace TypeElimination;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var Bar|null */
	private $bar;

	public function getValue(): string
	{

	}

	public function test()
	{
		/** @var Foo|null $foo */
		$foo = doFoo();

		if ($foo === null) {
			assertType('null', $foo);
		}

		if ($foo !== null) {
			assertType('TypeElimination\Foo', $foo);
		}

		if ($foo) {
			assertType('TypeElimination\Foo', $foo);
		} else {
			assertType('null', $foo);
		}

		if (!$foo) {
			assertType('null', $foo);
		} else {
			assertType('TypeElimination\Foo', $foo);
		}

		if (!$this->bar) {
			assertType('null', $this->bar);
		} else {
			assertType('TypeElimination\Bar', $this->bar);
		}

		if (null === $foo) {
			assertType('null', $foo);
		}

		if (null !== $foo) {
			assertType('TypeElimination\Foo', $foo);
		}

		/** @var int|false $intOrFalse */
		$intOrFalse = doFoo();
		if ($intOrFalse === false) {
			assertType('false', $intOrFalse);
		}

		if ($intOrFalse !== false) {
			assertType('int', $intOrFalse);
		}

		if (false === $intOrFalse) {
			assertType('false', $intOrFalse);
		}

		if (false !== $intOrFalse) {
			assertType('int', $intOrFalse);
		}

		if (!is_bool($intOrFalse)) {
			assertType('int', $intOrFalse);
		}

		/** @var int|true $intOrTrue */
		$intOrTrue = doFoo();
		if ($intOrTrue === true) {
			assertType('true', $intOrTrue);
		}

		if ($intOrTrue !== true) {
			assertType('int', $intOrTrue);
		}

		if (true === $intOrTrue) {
			assertType('true', $intOrTrue);
		}

		if (true !== $intOrTrue) {
			assertType('int', $intOrTrue);
		}

		if (!is_bool($intOrTrue)) {
			assertType('int', $intOrTrue);
		}

		/** @var Foo|Bar|Baz $fooOrBarOrBaz */
		$fooOrBarOrBaz = doFoo();
		if ($fooOrBarOrBaz instanceof Foo) {
			assertType('TypeElimination\Foo', $fooOrBarOrBaz);
		} else {
			assertType('TypeElimination\Bar|TypeElimination\Baz', $fooOrBarOrBaz);
		}

		if ($fooOrBarOrBaz instanceof Foo) {
			// already tested
		} elseif ($fooOrBarOrBaz instanceof Bar) {
			assertType('TypeElimination\Bar', $fooOrBarOrBaz);
		} else {
			assertType('TypeElimination\Baz', $fooOrBarOrBaz);
		}

		if (!$fooOrBarOrBaz instanceof Foo) {
			assertType('TypeElimination\Bar|TypeElimination\Baz', $fooOrBarOrBaz);
		} else {
			assertType('TypeElimination\Foo', $fooOrBarOrBaz);
		}

		/** @var Foo|string|null $value */
		$value = doFoo();
		$result = $value instanceof Foo ? $value->getValue() : $value;
		assertType('string|null', $result);

		/** @var Foo|string|null $fooOrStringOrNull */
		$fooOrStringOrNull = doFoo();
		if ($fooOrStringOrNull === null || $fooOrStringOrNull instanceof Foo) {
			assertType('TypeElimination\Foo|null', $fooOrStringOrNull);
			return;
		} else {
			assertType('string', $fooOrStringOrNull);
		}

		assertType('string', $fooOrStringOrNull);
	}

}
