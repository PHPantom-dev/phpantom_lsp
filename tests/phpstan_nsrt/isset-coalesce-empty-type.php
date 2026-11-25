<?php

namespace IssetCoalesceEmptyType;

use function PHPStan\Testing\assertType;

class FooEmpty
{

	public function doFoo()
	{
		$a = [];
		if (rand(0, 1)) {
			$a[0] = rand(0,1) ? true : false;
			$a[1] = false;
		}

		$a[2] = rand(0,1) ? true : false;
		$a[3] = false;
		$a[4] = true;

		assertType('bool', empty($a[0]));
		assertType('bool', empty($a[1]));
		assertType('bool', empty($a[2]));
	}

	public function doBar()
	{
		$a = [
			'',
			'0',
			'foo',
			rand(0, 1) ? '' : 'foo',
		];
		assertType('bool', empty($a[3]));
	}

	public function doBaz()
	{

		$b = 'test';

		if (rand(0, 1)) {
			$c = 'foo';
		}

		assertType('bool', empty($c));

		$d = rand(0, 1) ? '' : 'foo';
		assertType('bool', empty($d));
	}

}

class FooIsset
{
	/** @var string|null */
	public static $staticStringOrNull = null;

	/** @var string */
	public static $staticString = '';

	/** @var null */
	public static $staticAlwaysNull;

	/** @var string|null */
	public $stringOrNull = null;

	/** @var string */
	public $string = '';

	/** @var null */
	public $alwaysNull;

	/** @var FooIsset|null */
	public $FooIssetOrNull;

	/** @var FooIsset */
	public $FooIsset;

	public function thisCoalesce() {
	}

	function coalesce()
	{

		$scalar = 3;


		$array = [1, 2, 3];


		$multiDimArray = [[1], [2], [3]];



		if (rand() > 0.5) {
			$maybeVariable = 3;
		}

		assertType('bool', isset($maybeVariable));

		$fixedDimArray = [
			'dim' => 1,
			'dim-null' => rand() > 0.5 ? null : 1,
			'dim-null-offset' => ['a' => rand() > 0.5 ? true : null],
			'dim-empty' => []
		];

		// Always set

		// Maybe set
		assertType('bool', isset($fixedDimArray['dim-null']));

		// Never set, then unknown

		// Always set, then always set
		assertType('bool', isset($fixedDimArray['dim-null-offset']['a']));

		// Always set, then never set

		$foo = new FooIsset();

		assertType('bool', isset($foo->stringOrNull));




		assertType('bool', isset($foo->FooIssetOrNull->string));

		assertType('bool', isset(FooIsset::$staticStringOrNull));


	}

	/**
	 * @param array<string, int> $array
	 */
	function coalesceStringOffset(array $array)
	{
		assertType('bool', isset($array['string']));
	}

	function alwaysNullCoalesce (?string $a): void
	{
		if (!is_string($a)) {
		}
	}

	function fooo(): void {
		assertType('bool', isset((new FooIsset())->stringOrNull));
	}

	function fooooo(FooIsset $foo): void
	{
		assertType('bool', isset($foo::$staticStringOrNull));
	}
}

/**
 * @property int $integerProperty
 * @property FooIsset $foo
 */
class SomeMagicProperties
{

	function doFoo(SomeMagicProperties $foo, \stdClass $std): void {
		assertType('bool', isset($foo->integerProperty));

		assertType('bool', isset($foo->foo->string));

		assertType('bool', isset($std->foo));
	}

	function numericStringOffset(string $code): string
	{
		$array = [1, 2, 3];
		assertType('bool', isset($array[$code]));

		if (isset($array[$code])) {
			return (string) $array[$code];
		}

		$mappings = [
			'21021200' => '21028800',
		];

		assertType('bool', isset($mappings[$code]));

		if (isset($mappings[$code])) {
			return (string) $mappings[$code];
		}

		throw new \RuntimeException();
	}


	/**
	 * @param array{foo: string} $array
	 * @param 'bar' $bar
	 */
	function offsetFromPhpdoc(array $array, string $bar)
	{

		$array = ['bar' => 1];
	}


}

class FooNativeProp
{

	public int $hasDefaultValue = 0;

	public int $isAssignedBefore;

	public int $canBeUninitialized;

	function doFoo(FooNativeProp $foo): void {
		assertType('bool', isset($foo->hasDefaultValue));

		$foo->isAssignedBefore = 5;

		assertType('bool', isset($foo->canBeUninitialized));
	}

}

class Bug4290Isset
{
	public function test(): void
	{
		$array = self::getArray();

		assertType('bool', isset($array['status']));
		assertType('bool', isset($array['value']));

		$data = array_filter([
			'status' => isset($array['status']) ? $array['status'] : null,
			'value' => isset($array['value']) ? $array['value'] : null,
		]);

		if (count($data) === 0) {
			return;
		}

		assertType('bool', isset($data['status']));

		isset($data['status']) ? 1 : 0;
	}

	/**
	 * @return string[]
	 */
	public static function getArray(): array
	{
		return ['value' => '100'];
	}
}

class Bug4671
{

	/**
	 * @param array<string, string> $strings
	 */
	public function doFoo(int $intput, array $strings): void
	{
		assertType('bool', isset($strings[(string) $intput]));
	}

}

class MoreIsset
{

	function one()
	{

		/** @var string|null $alwaysDefinedNullable */
		$alwaysDefinedNullable = doFoo();

		assertType('bool', isset($alwaysDefinedNullable));

		$alwaysDefinedNotNullable = 'string';

		if (doFoo()) {
			$sometimesDefinedVariable = 1;
		}

		assertType('bool', isset(
			$sometimesDefinedVariable // fine, this is what's isset() is for
		));

		assertType('false', isset(
			$sometimesDefinedVariable, // fine, this is what's isset() is for
			$neverDefinedVariable // always false
		));

		assertType('bool', isset( // PHPStan says the literal bool; PHPantom types literal bools as `bool`
			$neverDefinedVariable // always false
		));

		/** @var array|null $anotherAlwaysDefinedNullable */
		$anotherAlwaysDefinedNullable = doFoo();

		assertType('bool', isset($anotherAlwaysDefinedNullable['test']['test']));

		/** @var array $anotherAlwaysDefinedNotNullable */
		$anotherAlwaysDefinedNotNullable = doFoo();
		assertType('bool', isset($anotherAlwaysDefinedNotNullable['test']['test']));



		assertType('bool', isset($_COOKIE['test']));


		if (doFoo()) {
			$yetAnotherVariableThatSometimesExists = 1;
		}

		assertType('bool', isset($yetAnotherVariableThatSometimesExists));

		/** @var string|null $nullableVariableUsedInTernary */
		$nullableVariableUsedInTernary = doFoo();
		assertType('bool', isset($nullableVariableUsedInTernary));
	}

	function two() {
		$alwaysDefinedNotNullable = 'string';
		if (doFoo()) {
			$sometimesDefinedVariable = 1;
		}

		assertType('false', isset(
			$alwaysDefinedNotNullable, // always true
			$sometimesDefinedVariable, // fine, this is what's isset() is for
			$neverDefinedVariable // always false
		));

		assertType('bool', isset( // PHPStan says the literal bool; PHPantom types literal bools as `bool`
			$alwaysDefinedNotNullable // always true
		));

		assertType('bool', isset(
			$alwaysDefinedNotNullable, // always true
			$sometimesDefinedVariable // fine, this is what's isset() is for
		));
	}

	function three() {
		$null = null;

	}

	function four() {
		assertType('bool', isset($_SESSION));
		assertType('bool', isset($_SESSION['foo']));
	}

}

class FooCoalesce
{
	/** @var string|null */
	public static $staticStringOrNull = null;

	/** @var string */
	public static $staticString = '';

	/** @var null */
	public static $staticAlwaysNull;

	/** @var string|null */
	public $stringOrNull = null;

	/** @var string */
	public $string = '';

	/** @var null */
	public $alwaysNull;

	/** @var FooCoalesce|null */
	public $fooCoalesceOrNull;

	/** @var FooCoalesce */
	public $fooCoalesce;

	public function thisCoalesce() {
		assertType('string', $this->string ?? false); // SKIP: `??` keeps its right side when the left side cannot be null
	}

	function coalesce()
	{

		$scalar = 3;

		assertType('3', $scalar ?? 4); // SKIP: `??` keeps its right side when the left side cannot be null

		$array = [1, 2, 3];

		assertType('0', $array['string'] ?? 0); // SKIP: `??` on an offset or variable that cannot exist gives `mixed`

		$multiDimArray = [[1], [2], [3]];

		assertType('0', $multiDimArray['string'] ?? 0); // SKIP: `??` on an offset or variable that cannot exist gives `mixed`

		assertType('0', $doesNotExist ?? 0);

		if (rand() > 0.5) {
			$maybeVariable = 3;
		}

		assertType('0|3', $maybeVariable ?? 0);

		$fixedDimArray = [
			'dim' => 1,
			'dim-null' => rand() > 0.5 ? null : 1,
			'dim-null-offset' => ['a' => rand() > 0.5 ? true : null],
			'dim-empty' => []
		];

		// Always set
		assertType('1', $fixedDimArray['dim'] ?? 0); // SKIP: `??` keeps its right side when the left side cannot be null

		// Maybe set
		assertType('0|1', $fixedDimArray['dim-null'] ?? 0);

		// Never set, then unknown
		assertType('0', $fixedDimArray['dim-null-not-set']['a'] ?? 0); // SKIP: `??` on an offset or variable that cannot exist gives `mixed`

		// Always set, then always set
		assertType('0|true', $fixedDimArray['dim-null-offset']['a'] ?? 0);

		// Always set, then never set
		assertType('0', $fixedDimArray['dim-empty']['b'] ?? 0);

		assertType('int<0, max>', rand() ?? false); // SKIP: `??` keeps its right side when the left side cannot be null


		$foo = new FooCoalesce();

		assertType('string|false', $foo->stringOrNull ?? false);

		assertType('string', $foo->string ?? false); // SKIP: `??` keeps its right side when the left side cannot be null

		assertType('\'\'', $foo->alwaysNull ?? '');

		assertType('string', $foo->fooCoalesce->string ?? false); // SKIP: `??` keeps its right side when the left side cannot be null

		assertType('string|false', $foo->fooCoalesceOrNull->string ?? false);

		assertType('string|false', FooCoalesce::$staticStringOrNull ?? false);

		assertType('string', FooCoalesce::$staticString ?? false); // SKIP: `??` keeps its right side when the left side cannot be null

		assertType('false', FooCoalesce::$staticAlwaysNull ?? false);
	}

	/**
	 * @param array<string, int> $array
	 */
	function coalesceStringOffset(array $array)
	{
		assertType('int|false', $array['string'] ?? false);
	}

	function alwaysNullCoalesce (?string $a): void
	{
		if (!is_string($a)) {
			assertType('false', $a ?? false);
		}
	}

	function foo(): void {
		assertType('string', (new FooCoalesce())->string ?? false); // SKIP: `??` keeps its right side when the left side cannot be null
		assertType('string|false', (new FooCoalesce())->stringOrNull ?? false);
		assertType('false', (new FooCoalesce())->alwaysNull ?? false);

		assertType(FooCoalesce::class, (new FooCoalesce()) ?? false);
		assertType('\'foo\'', null ?? 'foo');
	}

	function bar(FooCoalesce $foo): void
	{
		assertType('false', $foo::$staticAlwaysNull ?? false); // SKIP: a static property read through `$object::` has no type
		assertType('string', $foo::$staticString ?? false); // SKIP: a static property read through `$object::` has no type
		assertType('string|false', $foo::$staticStringOrNull ?? false); // SKIP: a static property read through `$object::` has no type
	}

	function lorem(): void {
		assertType('\'foo\'', $foo ?? 'foo');
		assertType('\'foo\'', $bar->bar ?? 'foo'); // SKIP: `??` on an offset or variable that cannot exist gives `mixed`
	}

	function ipsum(): void {
		$scalar = 3;
		assertType('3', $scalar ?? 4); // SKIP: `??` keeps its right side when the left side cannot be null
		assertType('0', $doesNotExist ?? 0);
	}

	function ipsum2(?string $a): void {
		if (!is_string($a)) {
			assertType('\'foo\'', $a ?? 'foo');
		}
	}

}
