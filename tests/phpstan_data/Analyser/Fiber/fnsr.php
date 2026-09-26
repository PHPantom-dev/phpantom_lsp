<?php // lint >= 8.1

declare(strict_types = 1);

namespace FiberNodeScopeResolverTest;

use Closure;
use DivisionByZeroError;
use function PHPStan\Testing\assertNativeType;
use function PHPStan\Testing\assertType;
use const PHP_VERSION_ID;

class Foo
{

	public function doFoo(int $i): ?string
	{
		return 'foo';
	}

	public function doImplicitArrayCreation(): void
	{
		$a['bla'] = 1;
		assertType('array{bla: 1}', $a); // SKIP: writing a literal into an array offset widens it to its base type
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doPlus($a, $b, int $c, int $d): void
	{
		assertType('int', $a + $b);
		assertNativeType('(array|float|int)', $a + $b);
		assertType('2', 1 + 1);
		assertNativeType('2', 1 + 1);
		assertType('int', $c + $d);
		assertNativeType('int', $c + $d);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doDiv($a, $b, int $c, int $d): void
	{
		assertType('(float|int)', $a / $b);
		assertNativeType('(float|int)', $a / $b);
		assertType('1', 1 / 1);
		assertNativeType('1', 1 / 1);
		assertType('(float|int)', $c / $d);
		assertNativeType('(float|int)', $c / $d);

	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doMod($a, $b, int $c, int $d): void
	{
		assertType('int', $a % $b);
		assertNativeType('int', $a % $b);
		assertType('0', 1 % 1);
		assertNativeType('0', 1 % 1);
		assertType('int', $c % $d);
		assertNativeType('int', $c % $d);

	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doMinus($a, $b, int $c, int $d): void
	{
		assertType('int', $a - $b);
		assertNativeType('(float|int)', $a - $b);
		assertType('0', 1 - 1);
		assertNativeType('0', 1 - 1);
		assertType('int', $c - $d);
		assertNativeType('int', $c - $d);
	}

	/**
	 * @param int $a
	 * @return void
	 */
	public function doBitwiseNot($a, int $b): void
	{
		assertType('int', ~$a);
		assertNativeType('int', ~$b);
		assertType('-2', ~1);
		assertNativeType('-2', ~1);
		assertType('int', ~$b);
		assertNativeType('int', ~$b);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doBitwiseAnd($a, $b, int $c, int $d): void
	{
		assertType('int', $a & $b);
		assertNativeType('(int|string)', $a & $b);
		assertType('1', 1 & 1);
		assertNativeType('1', 1 & 1);
		assertType('int', $c & $d);
		assertNativeType('int', $c & $d);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doBitwiseOr($a, $b, int $c, int $d): void
	{
		assertType('int', $a | $b);
		assertNativeType('(int|string)', $a | $b);
		assertType('1', 1 | 1);
		assertNativeType('1', 1 | 1);
		assertType('int', $c | $d);
		assertNativeType('int', $c | $d);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doBitwiseXor($a, $b, int $c, int $d): void
	{
		assertType('int', $a ^ $b);
		assertNativeType('(int|string)', $a ^ $b);
		assertType('0', 1 ^ 1);
		assertNativeType('0', 1 ^ 1);
		assertType('int', $c ^ $d);
		assertNativeType('int', $c ^ $d);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doMul($a, $b, int $c, int $d): void
	{
		assertType('int', $a * $b);
		assertNativeType('(float|int)', $a * $b);
		assertType('1', 1 * 1);
		assertNativeType('1', 1 * 1);
		assertType('int', $c * $d);
		assertNativeType('int', $c * $d);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doPow($a, $b, int $c, int $d): void
	{
		assertType('(float|int)', $a ** $b);
		assertNativeType('(float|int)', $a ** $b);
		assertType('1', 1 ** 1);
		assertNativeType('1', 1 ** 1);
		assertType('(float|int)', $c ** $d);
		assertNativeType('(float|int)', $c ** $d);
	}

	/**
	 * @param string $a
	 * @param string $b
	 * @return void
	 */
	public function doConcat($a, $b, string $c, string $d): void
	{
		assertType('string', $a . $b);
		assertNativeType('string', $a . $b);
		assertType("'1a'", '1' . 'a');
		assertNativeType("'1a'", '1' . 'a');
		assertType('string', $c . $d);
		assertNativeType('string', $c . $d);
	}

	/**
	 * @param int $ii
	 */
	function doUnaryPlus(int $i, $ii)
	{
		$a = '1';

		assertType('1', +$a);
		assertNativeType('1', +$a);
		assertType('int', +$i);
		assertNativeType('int', +$i);
		assertType('int', +$ii);
		assertNativeType('float|int', +$ii);
	}

	function doUnaryMinus(int $i) {
		$a = '1';

		assertType('-1', -$a);
		assertNativeType('-1', -$a);
		assertType('int', -$i);
		assertNativeType('int', -$i);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doShiftLeft($a, $b, int $c, int $d): void
	{
		assertType('int', $a << $b);
		assertNativeType('(float|int)', $a << $b);
		assertType('8', 1 << 3);
		assertNativeType('8', 1 << 3);
		assertType('int', $c << $d);
		assertNativeType('int', $c << $d);
	}

	/**
	 * @param int $a
	 * @param int $b
	 * @return void
	 */
	public function doShiftRight($a, $b, int $c, int $d): void
	{
		assertType('int', $a >> $b);
		assertNativeType('(float|int)', $a >> $b);
		assertType('0', 1 >> 3);
		assertNativeType('0', 1 >> 3);
		assertType('int', $c >> $d);
		assertNativeType('int', $c >> $d);
	}

	/**
	 * @param string $a
	 * @param string $b
	 * @return void
	 */
	public function doSpaceship($a, $b, string $c, string $d): void
	{
		assertNativeType('int<-1, 1>', $a <=> $b);
		assertType('-1', '1' <=> 'a');
		assertNativeType('-1', '1' <=> 'a');
		assertNativeType('int<-1, 1>', $c <=> $d);
	}

	function doCast() {
		$a = '1';

		assertType('1', (int) $a);
		assertType("array{'1'}", (array) $a);
		// PHPantom is more precise than PHPStan here: casting a scalar to object stores it in a `scalar` property.
		assertType('object{scalar: string}&stdClass', (object) $a);
		assertType('1.0', (double) $a);
		assertType("'1'", (string) $a);

		$f = 1.1;
		assertType('1', (int) $f);
	}

	/**
	 * @param '1' $b
	 */
	function doInterpolatedString(string $b) {
		$a = '1';

		assertType("'1'", "$a");
		assertNativeType("'1'", "$a");
		assertType("'1'", "$b");
		assertNativeType("string", "$b");
	}


}

function (): void {
	$foo = new Foo();
	assertType(Foo::class, $foo);
	assertType('string|null', $foo->doFoo(1));
	assertType($a = '1', (int) $a);
};

function (): void {
	assertType('array{foo: \'bar\'}', ['foo' => 'bar']);
	$a = [];
	assertType('array{}', $a);

};

function (): void {
	$a['bla'] = 1;
	assertType('array{bla: 1}', $a); // SKIP: writing a literal into an array offset widens it to its base type
};

function (): void {
	$cb = fn () => 1;
	assertType('Closure(): 1', $cb);

	$cb = fn (string $s) => (int) $s;
	assertType('Closure(string): int', $cb);

	$cb = function () {
		return 1;
	};
	assertType('Closure(): 1', $cb); // SKIP: a closure's inferred return type is not kept in the signature of the variable holding it

	$a = 1;
	$cb = function () use (&$a) {
		return 1;
	};
	assertType('Closure(): 1', $cb); // SKIP: a closure's inferred return type is not kept in the signature of the variable holding it

	$cb = function (string $s) {
		return $s;
	};
	assertType('Closure(string): string', $cb); // SKIP: a closure's inferred return type is not kept in the signature of the variable holding it
};

function (): void {
	$a = 0;
	$cb = function () use (&$a): void {
		assertType('0|\'s\'', $a); // SKIP: a variable a closure captures by reference does not take on the closure's assignments
		$a = 's';
	};
	assertType('0|\'s\'', $a);
};

function (): void {
	$a = 0;
	$b = 0;
	$cb = function () use (&$a, $b): void {
		assertType('0', $b);
		$a = $a + 1;
		$b = 1;
	};
	assertType('0', $b);
};

function (): void {
	$a = 0;
	$cb = function () use (&$a): void {
		assertType('0|1', $a); // SKIP: a variable a closure captures by reference does not take on the closure's assignments
		$a = 1;
	};
	assertType('0|1', $a);
};

class FooWithStaticMethods
{

	public function doFoo(): void
	{
		assertType('FiberNodeScopeResolverTest\\FooWithStaticMethods', self::returnSelf());
		assertNativeType('FiberNodeScopeResolverTest\\FooWithStaticMethods', self::returnSelf());
		assertType('FiberNodeScopeResolverTest\\FooWithStaticMethods', self::returnPhpDocSelf());
		assertNativeType('mixed', self::returnPhpDocSelf());
	}

	public static function returnSelf(): self
	{

	}

	/**
	 * @return self
	 */
	public static function returnPhpDocSelf()
	{

	}

	/**
	 * @template T
	 * @param T $a
	 * @return T
	 */
	public static function genericStatic($a)
	{

	}

	public function doFoo2(): void
	{
		assertType('1', self::genericStatic(1));

		$s = 'FiberNodeScopeResolverTest\\FooWithStaticMethods';
		assertType('1', $s::genericStatic(1));
	}

	public function doIf(int $i): void {
		if ($i) {
		} else {
			assertType('0', $i); // SKIP: falsiness narrowing does not narrow to the falsy values
		}

		assertType('int', $i);
	}

}

class ClosureFromCallableExtension
{

	/**
	 * @param callable(string, int=): bool $cb
	 */
	public function doFoo(callable $cb): void
	{
		assertType('callable(string, int=): bool', $cb);
		assertType('Closure(string, int=): bool', Closure::fromCallable($cb)); // SKIP: Closure::fromCallable() does not carry the callable's signature
	}

}

/**
 * @template T
 */
class FooGeneric
{

	/**
	 * @param T $a
	 */
	public function __construct($a)
	{

	}

}

function (): void {
	$foo = new FooGeneric(5);
	assertType('FiberNodeScopeResolverTest\\FooGeneric<int>', $foo);
};

function (): void {
	$c = new /** @template T of int */ class(1, 2, 3) {
		/**
		 * @param T $i
		 */
		public function __construct(private int $i, private int $j, private int $k) {

		}
	};
};

class MagicConstUser {
	function doFoo(): void {
		assertType('471', __LINE__); // SKIP: a magic constant resolves to its base type rather than its value
		assertType("'FiberNodeScopeResolverTest'", __NAMESPACE__); // SKIP: a magic constant resolves to its base type rather than its value
		assertType("'FiberNodeScopeResolverTest\\\\MagicConstUser'", __CLASS__); // SKIP: a magic constant resolves to its base type rather than its value
		assertType("''", __TRAIT__); // SKIP: a magic constant resolves to its base type rather than its value
		assertType("'doFoo'", __FUNCTION__); // SKIP: a magic constant resolves to its base type rather than its value
		assertType("'FiberNodeScopeResolverTest\\\\MagicConstUser::doFoo'", __METHOD__); // SKIP: a magic constant resolves to its base type rather than its value
		assertType("''", __PROPERTY__); // SKIP: a magic constant resolves to its base type rather than its value
	}
}

function (): void {
};

function (int $i) {
	if ($i == null) {
		assertType('0', $i); // SKIP: falsiness narrowing does not narrow to the falsy values
	} else {
	}

	assertType('int', $i);
};

function (int $i) {
	if ($i == false) {
		assertType('0', $i); // SKIP: falsiness narrowing does not narrow to the falsy values
	} else {
	}

	assertType('int', $i);
};

function (int $i) {
	if (false == $i) {
		assertType('0', $i); // SKIP: falsiness narrowing does not narrow to the falsy values
	} else {
	}

	assertType('int', $i);
};

function (int $i) {
	if ($i == true) {
	} else {
		assertType('0', $i); // SKIP: falsiness narrowing does not narrow to the falsy values
	}

	assertType('int', $i);
};

function (int $i) {
	if (true == $i) {
	} else {
		assertType('0', $i); // SKIP: falsiness narrowing does not narrow to the falsy values
	}

	assertType('int', $i);
};

function (mixed $m) {
	if ($m == 0) {
		assertType('0|0.0|string|false|null', $m); // SKIP: falsiness narrowing does not narrow to the falsy values
	} else {
	}

	assertType('mixed', $m);
};

function (mixed $m) {
	if ($m != 0) {
	} else {
		assertType('0|0.0|string|false|null', $m); // SKIP: falsiness narrowing does not narrow to the falsy values
	}

	assertType('mixed', $m);
};

function (mixed $m) {
	if ($m == '') {
		assertType("0|0.0|''|false|null", $m); // SKIP: falsiness narrowing does not narrow to the falsy values
	} else {
	}

	assertType('mixed', $m);
};

function (array $a): void {
	if ($a == []) {
		assertType("array{}", $a);
	} else {
		assertType("non-empty-array", $a);
	}

	assertType("array", $a);
};

function (array $a): void {
	if (!($a == [])) {
		assertType("non-empty-array", $a);
	} else {
		assertType("array{}", $a);
	}

	assertType("array", $a);
};

function (array $a): void {
	if ($a != []) {
		assertType("non-empty-array", $a);
	} else {
		assertType("array{}", $a);
	}

	assertType("array", $a);
};

function (bool $b): void {
	assertType('bool', !$b);
};

function (mixed $m): void {
	if ((bool) $m) {
	} else {
		assertType("0|0.0|''|'0'|array{}|false|null", $m); // SKIP: falsiness narrowing does not narrow to the falsy values
	}

	assertType("mixed", $m);
};

function (mixed $m): void {
	if ((string) $m) {
		//assertType("non-empty-array", $m);
	} else {
		//assertType("non-empty-array", $m);
	}

	assertType("mixed", $m);
};

function (mixed $m): void {
	if ((int) $m) {
		//assertType("non-empty-array", $m);
	} else {
		//assertType("non-empty-array", $m);
	}

	assertType("mixed", $m);
};

function (mixed $m): void {
	if ((float) $m) {
		//assertType("non-empty-array", $m);
	} else {
		//assertType("non-empty-array", $m);
	}

	assertType("mixed", $m);
};

function (array $a): void {
	if (count($a) === -10) {
		assertType('*NEVER*', $a); // SKIP: exhausted narrowing leaves the last type standing instead of never
	} else {
		assertType('array', $a);
	}
	assertType('array', $a);
	if (count($a) === 0) {
		assertType('array{}', $a);
	} else {
		assertType('non-empty-array', $a);
	}
	if (count($a) !== 0) {
		assertType('non-empty-array', $a);
	} else {
		assertType('array{}', $a);
	}
};

function (string $s): void {
	if (strlen($s) === 1) {
	} else {
		assertType('string', $s);
	}
};
