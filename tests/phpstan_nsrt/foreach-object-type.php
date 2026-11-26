<?php

namespace ObjectType;

use function PHPStan\Testing\assertType;

interface MyKey
{

}

interface MyValue
{

}

interface MyIterator extends \Iterator
{

	public function key(): MyKey;

	public function current(): MyValue;

}

interface MyIteratorAggregate extends \IteratorAggregate
{

	public function getIterator(): MyIterator;

}

interface MyIteratorAggregateRecursive extends \IteratorAggregate
{

	public function getIterator(): MyIteratorAggregateRecursive;

}

function test(MyIterator $iterator, MyIteratorAggregate $iteratorAggregate, MyIteratorAggregateRecursive $iteratorAggregateRecursive)
{
	foreach ($iterator as $keyFromIterator => $valueFromIterator) {
		assertType('ObjectType\MyKey', $keyFromIterator);
		assertType('ObjectType\MyValue', $valueFromIterator);
	}

	foreach ($iteratorAggregate as $keyFromAggregate => $valueFromAggregate) {
		assertType('ObjectType\MyKey', $keyFromAggregate);
		assertType('ObjectType\MyValue', $valueFromAggregate);
	}

	foreach ($iteratorAggregateRecursive as $keyFromRecursiveAggregate => $valueFromRecursiveAggregate) {
		assertType('mixed', $keyFromRecursiveAggregate);
		assertType('mixed', $valueFromRecursiveAggregate);
	}
}
