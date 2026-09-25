<?php declare(strict_types = 1);

namespace Bug13809;

use function PHPStan\Testing\assertType;

/**
 * @param list<mixed> $list
 */
function foo(array $list): void
{
	foreach ($list as &$value) {
		$value = 'foo';
	}

	assertType('list<mixed>', $list);
}

/**
 * @param list<mixed> $list
 */
function bar(array $list): void
{
	foreach ($list as $key => &$value) {
		$value = 'foo';
	}

	assertType("list<'foo'>", $list); // SKIP: a foreach by-reference write does not change the array's element type
}

/**
 * @param list<mixed> $list
 */
function bar2(array $list): void
{
	foreach ($list as $key => &$value) {
		if (rand(0, 1)) {
			$value = 'foo';
		}
		$key = 'bar';
	}

	assertType("list<mixed>", $list);
}

/**
 * @param list<mixed> $list
 */
function bar3(array $list): void
{
	foreach ($list as &$value) {
		if (rand(0, 1)) {
			$value = 'foo';
		} else {
			$value = 'maybe';
		}
	}

	assertType("list<mixed>", $list); // could be list<'foo'|'maybe'>
}

/**
 * @param list<string> $list
 */
function baz(array $list): void
{
	foreach ($list as &$value) {
		$value = 'bar';
	}

	assertType('list<string>', $list);
}

/**
 * @param list<int> $list
 */
function qux(array $list): void
{
	foreach ($list as &$value) {
		$value = $value + 1;
	}

	assertType('list<int>', $list);
}
