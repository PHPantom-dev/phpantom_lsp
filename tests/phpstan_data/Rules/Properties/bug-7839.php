<?php declare(strict_types = 1);

namespace Bug7839;

use function PHPStan\Testing\assertType;

class A
{
	/** @var string|self */
	public $table;
}

class B extends A
{
}

class C extends B
{
	public $table = 'a';
}

function (A $a, B $b, C $c) {
	assertType('Bug7839\\A|string', $a->table);
	assertType('Bug7839\\A|string', $b->table); // SKIP: `self` in an inherited property's docblock names the class it is read through
	assertType('Bug7839\\A|string', $c->table); // SKIP: `self` in an inherited property's docblock names the class it is read through
};
