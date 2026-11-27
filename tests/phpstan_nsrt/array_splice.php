<?php

namespace ArraySplice;

use function PHPStan\Testing\assertType;

final class Foo
{
	/** @var bool */
	public $abc = false;

	/** @var string */
	public $def = 'def';
}

/**
 * @param array<int, int> $arr
 * @return void
 */
function insertViaArraySplice(array $arr): void
{
	$brr = $arr;
	$extract = array_splice($brr, 0, 0, 1);
	assertType('non-empty-list<int>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, [1]);
	assertType('non-empty-list<int>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, '');
	assertType('non-empty-list<\'\'|int>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, ['']);
	assertType('non-empty-list<\'\'|int>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, null);
	assertType('list<int>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, [null]);
	assertType('non-empty-list<int|null>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, new Foo());
	assertType('non-empty-list<bool|int|string>', $brr); // SKIP: an object cast to an array is a plain `array`, not its properties
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, [new \stdClass()]);
	assertType('non-empty-list<int|stdClass>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, false);
	assertType('non-empty-list<int|false>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0, 0, [false]);
	assertType('non-empty-list<int|false>', $brr);
	assertType('array{}', $extract);

	$brr = $arr;
	$extract = array_splice($brr, 0);
	assertType('array{}', $brr);
	assertType('list<int>', $extract);
}

function constantArrays(array $arr, array $arr2): void
{
	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 0, 1, ['hello']);
	assertType('array{0: \'hello\', b: \'bar\', 1: \'baz\'}', $arr);
	assertType('array{\'foo\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 1, 2, ['hello']);
	assertType('array{\'foo\', \'hello\'}', $arr);
	assertType('array{b: \'bar\', 0: \'baz\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 0, -1, ['hello']);
	assertType('array{\'hello\', \'baz\'}', $arr);
	assertType('array{0: \'foo\', b: \'bar\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 0, -2, ['hello']);
	assertType('array{0: \'hello\', b: \'bar\', 1: \'baz\'}', $arr);
	assertType('array{\'foo\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, -1, -1, ['hello']);
	assertType('array{0: \'foo\', b: \'bar\', 1: \'hello\', 2: \'baz\'}', $arr);
	assertType('array{}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, -2, -2, ['hello']);
	assertType('array{0: \'foo\', 1: \'hello\', b: \'bar\', 2: \'baz\'}', $arr);
	assertType('array{}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 99, 0, ['hello']);
	assertType('array{0: \'foo\', b: \'bar\', 1: \'baz\', 2: \'hello\'}', $arr); // PHPStan says `array{0: 'foo', b: 'bar', 1: 'baz'}`, but PHP appends a replacement past the end
	assertType('array{}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 1, 99, ['hello']);
	assertType('array{\'foo\', \'hello\'}', $arr);
	assertType('array{b: \'bar\', 0: \'baz\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, -99, 99, ['hello']);
	assertType('array{\'hello\'}', $arr);
	assertType('array{0: \'foo\', b: \'bar\', 1: \'baz\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 0, -99, ['hello']);
	assertType('array{0: \'hello\', 1: \'foo\', b: \'bar\', 2: \'baz\'}', $arr);
	assertType('array{}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, -2, 1, ['hello']);
	assertType('array{\'foo\', \'hello\', \'baz\'}', $arr);
	assertType('array{b: \'bar\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, -1, 1, ['hello']);
	assertType('array{0: \'foo\', b: \'bar\', 1: \'hello\'}', $arr);
	assertType('array{\'baz\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 0, null, ['hello']);
	assertType('array{\'hello\'}', $arr);
	assertType('array{0: \'foo\', b: \'bar\', 1: \'baz\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	$arr;
	$extract = array_splice($arr, 0);
	assertType('array{}', $arr);
	assertType('array{0: \'foo\', b: \'bar\', 1: \'baz\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	/** @var array<\stdClass> $arr2 */
	$arr;
	$extract = array_splice($arr, 1, 1, $arr2);
	assertType('non-empty-array<int<0, max>, \'baz\'|\'foo\'|stdClass>', $arr);
	assertType('array{b: \'bar\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	/** @var array<\stdClass> $arr2 */
	$arr;
	$extract = array_splice($arr, 0, 1, $arr2);
	assertType('non-empty-array<\'b\'|int<0, max>, \'bar\'|\'baz\'|stdClass>', $arr);
	assertType('array{\'foo\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	/** @var array{x: 'x', y?: 'y', 3: 66}|array{z: 'z', 5?: 77, 4: int} $arr2 */
	$arr;
	$extract = array_splice($arr, 0, 1, $arr2);
	assertType('array{0: \'x\'|\'z\', 1: \'y\'|int, 2: \'baz\'|int, b: \'bar\', 3?: \'baz\'}', $arr); // SKIP: replacement shapes with optional entries give a generic array, not one shape
	assertType('array{\'foo\'}', $extract);

	/** @var array{17: 'foo', b: 'bar', 19: 'baz'} $arr */
	/** @var array{x: 'x', y?: 'y', 3: 66}|array{z: 'z', 5?: 77, 4: int}|array<object|null> $arr2 */
	$arr;
	$extract = array_splice($arr, 0, 1, $arr2);
	assertType('non-empty-array<\'b\'|int<0, max>, \'bar\'|\'baz\'|\'x\'|\'y\'|\'z\'|int|object|null>', $arr);
	assertType('array{\'foo\'}', $extract);
}

function constantArraysWithOptionalKeys(array $arr): void
{
	/**
	 * @see https://3v4l.org/2UJ3u
	 * @var array{a?: 0, b: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 0, 1, ['hello']);
	assertType('array{0: \'hello\', b?: 1, c: 2}', $arr);
	assertType('array{a?: 0, b?: 1}', $extract);

	/**
	 * @see https://3v4l.org/Aq4l6
	 * @var array{a?: 0, b: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 1, 1, ['hello']);
	assertType('array{a?: 0, b?: 1, 0: \'hello\', c?: 2}', $arr);
	assertType('array{b?: 1, c?: 2}', $extract);

	/**
	 * @see https://3v4l.org/GBMps
	 * @var array{a?: 0, b: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, -1, 0, ['hello']);
	assertType('array{a?: 0, b: 1, 0: \'hello\', c: 2}', $arr);
	assertType('array{}', $extract);

	/**
	 * @see https://3v4l.org/dQVgY
	 * @var array{a?: 0, b: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 0, -1, ['hello']);
	assertType('array{0: \'hello\', c: 2}', $arr);
	assertType('array{a?: 0, b: 1}', $extract);

	/**
	 * @see https://3v4l.org/5XWRC
	 * @var array{a: 0, b?: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 0, 1, ['hello']);
	assertType('array{0: \'hello\', b?: 1, c: 2}', $arr);
	assertType('array{a: 0}', $extract);

	/**
	 * @see https://3v4l.org/QXZre
	 * @var array{a: 0, b?: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 1, 1, ['hello']);
	assertType('array{a: 0, 0: \'hello\', c?: 2}', $arr);
	assertType('array{b?: 1, c?: 2}', $extract);

	/**
	 * @see https://3v4l.org/4JvMu
	 * @var array{a: 0, b?: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, -1, 0, ['hello']);
	assertType('array{a: 0, b?: 1, 0: \'hello\', c: 2}', $arr);
	assertType('array{}', $extract);

	/**
	 * @see https://3v4l.org/srHon
	 * @var array{a: 0, b?: 1, c: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 0, -1, ['hello']);
	assertType('array{0: \'hello\', c: 2}', $arr);
	assertType('array{a: 0, b?: 1}', $extract);

	/**
	 * @see https://3v4l.org/d0b0c
	 * @var array{a: 0, b: 1, c?: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 0, 1, ['hello']);
	assertType('array{0: \'hello\', b: 1, c?: 2}', $arr);
	assertType('array{a: 0}', $extract);

	/**
	 * @see https://3v4l.org/OPfIf
	 * @var array{a: 0, b: 1, c?: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 1, 1, ['hello']);
	assertType('array{a: 0, 0: \'hello\', c?: 2}', $arr);
	assertType('array{b: 1}', $extract);

	/**
	 * @see https://3v4l.org/b9R9E
	 * @var array{a: 0, b: 1, c?: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, -1, 0, ['hello']);
	assertType('array{a: 0, b: 1, 0: \'hello\', c?: 2}', $arr);
	assertType('array{}', $extract);

	/**
	 * @see https://3v4l.org/0lFX6
	 * @var array{a: 0, b: 1, c?: 2} $arr
	 */
	$arr;
	$extract = array_splice($arr, 0, -1, ['hello']);
	assertType('array{0: \'hello\', b?: 1, c?: 2}', $arr);
	assertType('array{a: 0, b?: 1}', $extract);

	/**
	 * @see https://3v4l.org/PLHYv
	 * @var array{a: 0, b?: 1, c?: 2, d: 3} $arr
	 */
	$arr;
	$extract = array_splice($arr, 1, 2, ['hello']);
	assertType('array{a: 0, 0: \'hello\', d?: 3}', $arr);
	assertType('array{b?: 1, c?: 2, d?: 3}', $extract);

	/**
	 * @see https://3v4l.org/Li5bj
	 * @var array{a: 0, b?: 1, c?: 2, d: 3} $arr
	 */
	$arr;
	$extract = array_splice($arr, -2, 2, ['hello']);
	assertType('array{a?: 0, b?: 1, 0: \'hello\'}', $arr);
	assertType('array{a?: 0, b?: 1, c?: 2, d: 3}', $extract);
}

function offsets(array $arr): void
{
	if (array_key_exists(1, $arr)) {
		$extract = array_splice($arr, 0, 1, 'hello');
		assertType('non-empty-array<(int<0, max>|string), mixed>', $arr);
		assertType('non-empty-array', $extract); // PHPStan says `array`, but a slice from the start of a non-empty array keeps its first entry
	}

	if (array_key_exists(1, $arr)) {
		$extract = array_splice($arr, 0, 0, 'hello');
		assertType('non-empty-array<(int<0, max>|string), mixed>&hasOffset(1)', $arr); // SKIP: a key `array_key_exists()` proved does not survive `array_splice()`
		assertType('array{}', $extract);
	}

	if (array_key_exists(1, $arr) && $arr[1] === 'foo') {
		$extract = array_splice($arr, 0, 1, 'hello');
		assertType('non-empty-array<(int<0, max>|string), mixed>', $arr);
		assertType('non-empty-array<(int<0, max>|string), mixed>', $extract); // PHPStan says `array`, but a slice from the start of a non-empty array keeps its first entry
	}

	if (array_key_exists(1, $arr) && $arr[1] === 'foo') {
		$extract = array_splice($arr, 0, 0, 'hello');
		assertType('non-empty-array<(int<0, max>|string), mixed>&hasOffsetValue(1, \'foo\')', $arr); // SKIP: a key `array_key_exists()` proved does not survive `array_splice()`
		assertType('array{}', $extract);
	}
}

function lists(array $arr): void
{
	/** @var list<string> $arr */
	$arr;
	$extract = array_splice($arr, 0, 1, 'hello');
	assertType('non-empty-list<string>', $arr);
	assertType('list<string>', $extract);

	/** @var non-empty-list<string> $arr */
	$arr;
	$extract = array_splice($arr, 0, 1);
	assertType('list<string>', $arr);
	assertType('non-empty-list<string>', $extract);

	/** @var list<string> $arr */
	$arr;
	$extract = array_splice($arr, 0, 0, 'hello');
	assertType('non-empty-list<string>', $arr);
	assertType('array{}', $extract);

	/** @var list<string> $arr */
	$arr;
	$extract = array_splice($arr, 0, null, 'hello');
	assertType('non-empty-list<string>', $arr);
	assertType('list<string>', $extract);

	/** @var list<string> $arr */
	$arr;
	$extract = array_splice($arr, 0, null);
	assertType('array{}', $arr);
	assertType('list<string>', $extract);

	/** @var list<string> $arr */
	$arr;
	$extract = array_splice($arr, 0, 1);
	assertType('list<string>', $arr);
	assertType('list<string>', $extract);
}

/** @param array<string, string> $arr */
function more(array $arr): void
{
	$extract = array_splice($arr, 0, 1, [17 => 'foo', 18 => 'bar']);
	assertType('non-empty-array<0|1|string, string>', $arr);
	assertType('array<string, string>', $extract);
}
