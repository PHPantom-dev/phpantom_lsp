<?php

namespace Bug9274;

use SplDoublyLinkedList;
use SplQueue;
use function PHPStan\Testing\assertType;

class Point
{
	public int $x;
}

/** @extends SplDoublyLinkedList<Point> */
class A extends SplDoublyLinkedList {}
/** @extends SplQueue<Point> */
class B extends SplQueue {}

function testSplDoublyLinkedList(): void
{
	$dll = new A();
	$p1 = $dll[0];

	assertType('Bug9274\Point', $p1);
	assertType('int', $p1->x);
}

function testSplQueue(): void
{
	$queue = new B();
	$p2 = $queue[0];

	assertType('Bug9274\Point', $p2);
	assertType('int', $p2->x);
}
