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

	// More precise than upstream, which reports `list<mixed>`: every entry
	// was overwritten through the reference.
	assertType("list<'foo'>", $list);
}

/**
 * @param list<mixed> $list
 */
function bar(array $list): void
{
	foreach ($list as $key => &$value) {
		$value = 'foo';
	}

	assertType("list<'foo'>", $list);
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

	// More precise than upstream, which reports `list<mixed>`.
	assertType("list<'foo'|'maybe'>", $list);
}

/**
 * @param list<string> $list
 */
function baz(array $list): void
{
	foreach ($list as &$value) {
		$value = 'bar';
	}

	// More precise than upstream, which reports `list<string>`.
	assertType("list<'bar'>", $list);
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
