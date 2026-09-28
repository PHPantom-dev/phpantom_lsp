<?php

namespace UnionIntersection;

use function PHPStan\Testing\assertType;

class WithFoo
{

	/** @var 1 */
	const FOO_CONSTANT = 1;

	/** @var Foo */
	public $foo;

	public function doFoo(): Foo
	{

	}

	public static function doStaticFoo(): Foo
	{

	}

}

class WithFooAndBar
{

	/** @var 1 */
	const FOO_CONSTANT = 1;

	/** @var 1 */
	const BAR_CONSTANT = 1;

	/** @var AnotherFoo */
	public $foo;

	/** @var Bar */
	public $bar;

	public function doFoo(): AnotherFoo
	{

	}

	public static function doStaticFoo(): AnotherFoo
	{

	}

	public function doBar(): Bar
	{

	}

	public static function doStaticBar(): Bar
	{

	}

}

interface WithFooAndBarInterface
{

	/** @var 1 */
	const FOO_CONSTANT = 1;

	/** @var 1 */
	const BAR_CONSTANT = 1;

	public function doFoo(): AnotherFoo;

	public static function doStaticFoo(): AnotherFoo;

	public function doBar(): Bar;

	public static function doStaticBar(): Bar;

}

interface SomeInterface
{

}

class Dolor
{

	/** @var array{1, 2, 3} */
	const PARENT_CONSTANT = [1, 2, 3];

}

class Ipsum extends Dolor
{

	const IPSUM_CONSTANT = 'foo';

	/** @var WithFoo|WithFooAndBar */
	private $union;

	/** @var WithFoo|object */
	private $objectUnion;

	public function doFoo(WithFoo $foo, WithFoo $foobar, object $object)
	{
		if ($foo instanceof SomeInterface) {
			if ($foobar instanceof WithFooAndBarInterface) {
				if ($object instanceof SomeInterface) {
					assertType('UnionIntersection\AnotherFoo|UnionIntersection\Foo', $this->union->foo);
					assertType('UnionIntersection\Bar', $this->union->bar);
					assertType('UnionIntersection\Foo', $foo->foo);
					assertType('*ERROR*', $foo->bar);
					assertType('UnionIntersection\AnotherFoo|UnionIntersection\Foo', $this->union->doFoo());
					assertType('UnionIntersection\Bar', $this->union->doBar());
					assertType('UnionIntersection\Foo', $foo->doFoo());
					assertType('*ERROR*', $foo->doBar());
					assertType('UnionIntersection\AnotherFoo&UnionIntersection\Foo', $foobar->doFoo());
					assertType('UnionIntersection\Bar', $foobar->doBar());
					assertType('1', $this->union::FOO_CONSTANT);
					assertType('1', $this->union::BAR_CONSTANT);
					assertType('1', $foo::FOO_CONSTANT);
					assertType('*ERROR*', $foo::BAR_CONSTANT);
					assertType('1', $foobar::FOO_CONSTANT);
					assertType('1', $foobar::BAR_CONSTANT);
					assertType('\'foo\'', self::IPSUM_CONSTANT);
					// PHPantom spells a list-shaped constant array as list{…}.
					assertType('list{1, 2, 3}', parent::PARENT_CONSTANT);
					assertType('UnionIntersection\Foo', $foo::doStaticFoo());
					assertType('*ERROR*', $foo::doStaticBar());
					assertType('UnionIntersection\AnotherFoo&UnionIntersection\Foo', $foobar::doStaticFoo());
					assertType('UnionIntersection\Bar', $foobar::doStaticBar());
					assertType('UnionIntersection\AnotherFoo|UnionIntersection\Foo', $this->union::doStaticFoo());
					assertType('UnionIntersection\Bar', $this->union::doStaticBar());
					// PHPantom keeps the class a union with object names, since completion offers its members.
					assertType('UnionIntersection\WithFoo|object', $this->objectUnion);
					assertType('UnionIntersection\SomeInterface', $object);
				}
			}
		}
	}

}
