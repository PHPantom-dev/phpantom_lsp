<?php declare(strict_types = 1);

namespace Bug12376;

use function PHPStan\Testing\assertType;

/**
 * workaround https://github.com/phpstan/phpstan-src/pull/3853
 *
 * @template T of object
 * @param class-string<T> $class
 * @return T
 */
function newNonFinalInstance(string $class): object
{
	return new $class();
}

class Base {}

class A extends Base
{
	/**
	 * @template T of object
	 * @param T $object
	 * @return (T is static ? T : static)
	 *
	 * @phpstan-assert static $object
	 */
	public static function assertInstanceOf(object $object)
	{
		if (!$object instanceof static) {
			throw new \Exception();
		}

		return $object;
	}
}

class B extends A {}
class C extends Base {}

$o = newNonFinalInstance(\DateTime::class);
$r = A::assertInstanceOf($o);
assertType('*NEVER*', $o); // SKIP
assertType('Bug12376\A', $r);

$o = newNonFinalInstance(A::class);
$r = A::assertInstanceOf($o);
assertType('Bug12376\A', $o);
assertType('Bug12376\A', $r);

$o = newNonFinalInstance(B::class);
$r = A::assertInstanceOf($o);
assertType('Bug12376\B', $o);
assertType('Bug12376\B', $r);

$o = newNonFinalInstance(C::class);
$r = A::assertInstanceOf($o);
assertType('*NEVER*', $o); // SKIP
assertType('Bug12376\A', $r);

$o = newNonFinalInstance(A::class);
$r = B::assertInstanceOf($o);
assertType('Bug12376\B', $o);
assertType('Bug12376\B', $r);
