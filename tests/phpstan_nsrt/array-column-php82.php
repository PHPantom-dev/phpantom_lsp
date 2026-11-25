<?php // lint >= 8.2

namespace ArrayColumn82;

use DOMElement;
use function PHPStan\Testing\assertType;


class ArrayColumnTest
{

	/** @param array<int, array<string, string>> $array */
	public function testArray1(array $array): void
	{
		assertType('list<string>', array_column($array, 'column'));
		assertType('list<string>', array_column($array, 'column', null));
		assertType('array<int|string, string>', array_column($array, 'column', 'key'));
		assertType('array<int|string, array<string, string>>', array_column($array, null, 'key'));
	}

	/** @param non-empty-array<int, array<string, string>> $array */
	public function testArray2(array $array): void
	{
		// Note: Array may still be empty!
		assertType('list<string>', array_column($array, 'column'));
		assertType('list<string>', array_column($array, 'column', null));
	}

	/** @param array{} $array */
	public function testArray3(array $array): void
	{
		assertType('array{}', array_column($array, 'column'));
		assertType('array{}', array_column($array, 'column', null));
		assertType('array{}', array_column($array, 'column', 'key'));
		assertType('array{}', array_column($array, null, 'key'));
	}

	/** @param array<int, array<string, float>> $array */
	public function testArray4(array $array): void
	{
		assertType('array<int, float>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, array<string, bool>> $array */
	public function testArray5(array $array): void
	{
		assertType('array<int, bool>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, array<string, true>> $array */
	public function testArray6(array $array): void
	{
		assertType('array<int, true>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, array<string, null>> $array */
	public function testArray7(array $array): void
	{
		assertType('array<\'\'|int, null>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, array<string, array>> $array */
	public function testArray8(array $array): void
	{
		assertType('array<int, array>', array_column($array, 'column', 'key')); // SKIP: array_column() with an index column holding arrays returns `array`
	}

	/** @param array<int, array{column: string, key: string}> $array */
	public function testConstantArray1(array $array): void
	{
		assertType('list<string>', array_column($array, 'column'));
		assertType('list<string>', array_column($array, 'column', null));
		assertType('array<string, string>', array_column($array, 'column', 'key'));
		assertType('array<string, array{column: string, key: string}>', array_column($array, null, 'key'));
	}

	/** @param array<int, array{column: string, key: string}> $array */
	public function testConstantArray2(array $array): void
	{
		assertType('array{}', array_column($array, 'foo'));
		assertType('array{}', array_column($array, 'foo', null));
		assertType('array{}', array_column($array, 'foo', 'key'));
	}

	/** @param array{array{column: string, key: 'bar'}} $array */
	public function testConstantArray3(array $array): void
	{
		assertType("array{string}", array_column($array, 'column'));
		assertType("array{string}", array_column($array, 'column', null));
		assertType("array{bar: string}", array_column($array, 'column', 'key'));
		assertType("array{bar: array{column: string, key: 'bar'}}", array_column($array, null, 'key'));
	}

	/** @param array{array{column: string, key: string}} $array */
	public function testConstantArray4(array $array): void
	{
		assertType("non-empty-array<string, string>", array_column($array, 'column', 'key'));
		assertType("non-empty-array<string, array{column: string, key: string}>", array_column($array, null, 'key'));
	}

	/** @param array<int, array{column?: 'foo', key?: 'bar'}> $array */
	public function testConstantArray5(array $array): void
	{
		assertType("list<'foo'>", array_column($array, 'column'));
		assertType("list<'foo'>", array_column($array, 'column', null));
		assertType("array<'bar'|int, 'foo'>", array_column($array, 'column', 'key'));
		assertType("array<'bar'|int, array{column?: 'foo', key?: 'bar'}>", array_column($array, null, 'key'));
	}

	/** @param array<int, array{column1: string, column2: bool}> $array */
	public function testConstantArray6(array $array): void
	{
		assertType('list<bool|string>', array_column($array, mt_rand(0, 1) === 0 ? 'column1' : 'column2'));
		assertType('list<bool|string>', array_column($array, mt_rand(0, 1) === 0 ? 'column1' : 'column2', null));
	}

	/** @param non-empty-array<int, array{column: string, key: string}> $array */
	public function testConstantArray7(array $array): void
	{
		assertType('non-empty-list<string>', array_column($array, 'column'));
		assertType('non-empty-list<string>', array_column($array, 'column', null));
		assertType('non-empty-array<string, string>', array_column($array, 'column', 'key'));
		assertType('non-empty-array<string, array{column: string, key: string}>', array_column($array, null, 'key'));
	}

	/** @param array<int, array{column: string, key: float}> $array */
	public function testConstantArray8(array $array): void
	{
		assertType('array<int, string>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, array{column: string, key: bool}> $array */
	public function testConstantArray9(array $array): void
	{
		assertType('array<0|1, string>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, array{column: string, key: true}> $array */
	public function testConstantArray10(array $array): void
	{
		assertType('array<1, string>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, array{column: string, key: null}> $array */
	public function testConstantArray11(array $array): void
	{
		assertType('array<\'\', string>', array_column($array, 'column', 'key'));
	}

	/** @param array{0?: array{column: 'foo', key: 'bar'}} $array */
	public function testConstantArray12(array $array): void
	{
		assertType("array{0?: 'foo'}", array_column($array, 'column'));
		assertType("array{0?: 'foo'}", array_column($array, 'column', null));
		assertType("array{bar?: 'foo'}", array_column($array, 'column', 'key'));
	}

	// These cases aren't handled precisely and will return non-constant arrays.

	/** @param array{array{column?: 'foo', key: 'bar'}} $array */
	public function testImprecise1(array $array): void
	{
		assertType("list<'foo'>", array_column($array, 'column'));
		assertType("list<'foo'>", array_column($array, 'column', null));
		assertType("array<'bar', 'foo'>", array_column($array, 'column', 'key'));
		assertType("array{bar: array{column?: 'foo', key: 'bar'}}", array_column($array, null, 'key'));
	}

	/** @param array{array{column: 'foo', key?: 'bar'}} $array */
	public function testImprecise2(array $array): void
	{
		assertType("non-empty-array<'bar'|int, 'foo'>", array_column($array, 'column', 'key'));
		assertType("non-empty-array<'bar'|int, array{column: 'foo', key?: 'bar'}>", array_column($array, null, 'key'));
	}

	/** @param array{array{column: 'foo', key: 'bar'}}|array<int, array<string, string>> $array */
	public function testImprecise3(array $array): void
	{
		assertType('list<string>', array_column($array, 'column'));
		assertType('list<string>', array_column($array, 'column', null));
		assertType('array<int|string, string>', array_column($array, 'column', 'key'));
	}

	/** @param array<int, DOMElement> $array */
	public function testImprecise5(array $array): void
	{
		assertType('list<string>', array_column($array, 'nodeName')); // SKIP: array_column() does not read object rows
		assertType('list<string>', array_column($array, 'nodeName', null)); // SKIP: array_column() does not read object rows
		assertType('array<string, string>', array_column($array, 'nodeName', 'tagName')); // SKIP: array_column() does not read object rows
		assertType('array<string, DOMElement>', array_column($array, null, 'tagName')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($array, 'foo')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($array, 'foo', null)); // SKIP: array_column() does not read object rows
		assertType('array<string, mixed>', array_column($array, 'foo', 'tagName')); // SKIP: array_column() does not read object rows
		assertType('array<string>', array_column($array, 'nodeName', 'foo')); // SKIP: array_column() does not read object rows
		assertType('array<DOMElement>', array_column($array, null, 'foo')); // SKIP: array_column() does not read object rows
	}

	/** @param non-empty-array<int, DOMElement> $array */
	public function testObjects1(array $array): void
	{
		assertType('non-empty-list<string>', array_column($array, 'nodeName')); // SKIP: array_column() does not read object rows
		assertType('non-empty-list<string>', array_column($array, 'nodeName', null)); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<string, string>', array_column($array, 'nodeName', 'tagName')); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<string, DOMElement>', array_column($array, null, 'tagName')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($array, 'foo')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($array, 'foo', null)); // SKIP: array_column() does not read object rows
		assertType('array<string, mixed>', array_column($array, 'foo', 'tagName')); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<string>', array_column($array, 'nodeName', 'foo')); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<DOMElement>', array_column($array, null, 'foo')); // SKIP: array_column() does not read object rows
	}

	/** @param array{DOMElement} $array */
	public function testObjects2(array $array): void
	{
		assertType('array{string}', array_column($array, 'nodeName')); // SKIP: array_column() does not read object rows
		assertType('array{string}', array_column($array, 'nodeName', null)); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<string, string>', array_column($array, 'nodeName', 'tagName')); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<string, DOMElement>', array_column($array, null, 'tagName')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($array, 'foo')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($array, 'foo', null)); // SKIP: array_column() does not read object rows
		assertType('array<string, mixed>', array_column($array, 'foo', 'tagName')); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<string>', array_column($array, 'nodeName', 'foo')); // SKIP: array_column() does not read object rows
		assertType('non-empty-array<DOMElement>', array_column($array, null, 'foo')); // SKIP: array_column() does not read object rows
	}

}

final class Foo
{

	/** @param array<int, self> $a */
	public function doFoo(array $a): void
	{
		assertType('array{}', array_column($a, 'nodeName')); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($a, 'nodeName', null)); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($a, 'nodeName', 'tagName')); // SKIP: array_column() does not read object rows
	}

}

class NonFinalObjectWithVisibility
{
	public int $pub = 1;
	protected int $prot = 2;
	private int $priv = 3;
}

final class FinalObjectWithVisibility
{
	public int $pub = 1;
	protected int $prot = 2;
	private int $priv = 3;
}

class ArrayColumnVisibilityNonFinalTest
{

	/** @param array<int, NonFinalObjectWithVisibility> $objects */
	public function testNonFinal(array $objects): void
	{
		assertType('list<int>', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('list<int>', array_column($objects, 'prot')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

	/** @param array{NonFinalObjectWithVisibility} $objects */
	public function testNonFinalConstant(array $objects): void
	{
		assertType('array{int}', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('list<int>', array_column($objects, 'prot')); // Could be array{}|array{int} // SKIP: array_column() does not read object rows
		assertType('list', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

}

class ArrayColumnVisibilityFinalTest
{

	/** @param array<int, FinalObjectWithVisibility> $objects */
	public function testFinal(array $objects): void
	{
		assertType('list<int>', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($objects, 'prot')); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

	/** @param array{FinalObjectWithVisibility} $objects */
	public function testFinalConstant(array $objects): void
	{
		assertType('array{int}', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($objects, 'prot')); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

	/** @param array<int, FinalObjectWithVisibility> $objects */
	public function testNonPublicAsIndex(array $objects): void
	{
		assertType('array<int, int>', array_column($objects, 'pub', 'pub')); // SKIP: array_column() does not read object rows
		assertType('array<int, int>', array_column($objects, 'pub', 'priv')); // SKIP: array_column() does not read object rows
	}

}

class ArrayColumnVisibilityImplicitFinalTest
{

	public function testNewExpression(): void
	{
		$objects = [new NonFinalObjectWithVisibility()];
		assertType('array{int}', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($objects, 'prot')); // SKIP: array_column() does not read object rows
		assertType('array{}', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

}

class ArrayColumnVisibilityFromInsideTest
{

	public int $pub = 1;
	private int $priv = 2;

	/** @param list<self> $objects */
	public function testFromInside(array $objects): void
	{
		assertType('list<int>', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('list<int>', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

}

class ArrayColumnVisibilityFromChildTest extends NonFinalObjectWithVisibility
{

	/** @param list<NonFinalObjectWithVisibility> $objects */
	public function testFromChild(array $objects): void
	{
		assertType('list<int>', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('list<int>', array_column($objects, 'prot')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

}

final class ObjectWithIssetOnly
{
	private int $priv = 2;

	public function __isset(string $name): bool
	{
		return true;
	}
}

class ArrayColumnVisibilityWithIssetOnlyTest
{

	/** @param array<int, ObjectWithIssetOnly> $objects */
	public function testWithIssetOnly(array $objects): void
	{
		assertType('array{}', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

}

class ObjectWithIsset
{
	public int $pub = 1;
	private int $priv = 2;

	public function __isset(string $name): bool
	{
		return true;
	}

	public function __get(string $name): mixed
	{
		return $this->$name;
	}
}

class ArrayColumnVisibilityWithIssetTest
{

	/** @param array<int, ObjectWithIsset> $objects */
	public function testWithIsset(array $objects): void
	{
		assertType('list<int>', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

	/** @param array{ObjectWithIsset} $objects */
	public function testWithIssetConstant(array $objects): void
	{
		assertType('array{int}', array_column($objects, 'pub')); // SKIP: array_column() does not read object rows
		assertType('list', array_column($objects, 'priv')); // SKIP: array_column() does not read object rows
	}

}
