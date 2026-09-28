<?php

namespace Bug3769;

use function PHPStan\Testing\assertType;

/**
 * @template K of array-key
 * @param array<K, int> $in
 * @return array<K, string>
 */
function stringValues(array $in): array {
	$a = assertType('array<K of (int|string) (function Bug3769\stringValues(), argument), int>', $in);
	return array_map(function (int $int): string {
		return (string) $int;
	}, $in);
}

/**
 * @param array<int, int> $foo
 * @param array<string, int> $bar
 * @param array<int> $baz
 */
function foo(
	array $foo,
	array $bar,
	array $baz
): void {
	$a = assertType('array<int, string>', stringValues($foo));
	$a = assertType('array<string, string>', stringValues($bar));
	$a = assertType('array<string>', stringValues($baz));
	echo 'test';
};

/**
 * @template T of \stdClass|\Exception
 * @param T $foo
 */
function fooUnion($foo): void {
	$a = assertType('T of Exception|stdClass (function Bug3769\fooUnion(), argument)', $foo);
	echo 'test';
}

/**
 * @template T
 * @param T $a
 * @return T
 */
function mixedBound($a)
{
	return $a;
}

/**
 * @template T of int
 * @param T $a
 * @return T
 */
function intBound(int $a)
{
	return $a;
}

/**
 * @template T of string
 * @param T $a
 * @return T
 */
function stringBound(string $a)
{
	return $a;
}

function (): void {
	$a = assertType('1', mixedBound(1));
	$a = assertType('\'str\'', mixedBound('str'));
	$a = assertType('1', intBound(1));
	$a = assertType('\'str\'', stringBound('str'));
};

/** @template T of string */
class Foo
{

	/** @var T */
	private $value;

	/**
	 * @param T $value
	 */
	public function __construct($value)
	{
		$this->value = $value;
	}

	/**
	 * @return T
	 */
	public function getValue()
	{
		return $this->value;
	}

}

/** @param Foo<'bar'> $foo */
function testTofString(Foo $foo): void {
	$a = assertType('\'bar\'', $foo->getValue());

	$baz = new Foo('baz');
	$a = assertType('\'baz\'', $baz->getValue());
};
