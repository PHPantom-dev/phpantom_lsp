<?php

namespace ArrayUnshift;

use stdClass;

use function array_unshift;
use function PHPStan\Testing\assertType;

/**
 * @param string[] $a
 * @param int[] $b
 * @param non-empty-array<int> $c
 * @param array<int|string> $d
 * @param list<string> $e
 */
function arrayUnshift(array $a, array $b, array $c, array $d, array $e, array $arr): void
{
	array_unshift($a, ...$b);
	assertType('array<int|string>', $a);

	/** @var non-empty-array<string> $arr */
	array_unshift($arr, ...$b);
	assertType('non-empty-array<int|string>', $arr);

	array_unshift($b, ...[]);
	assertType('array<int>', $b);

	array_unshift($c, ...[19, 'baz', false]);
	assertType('non-empty-array<\'baz\'|int|false>', $c);

	/** @var array<bool|null> $d1 */
	$d1 = [];
	array_unshift($d, ...$d1);
	assertType('array<bool|int|string|null>', $d);

	/** @var list<bool> $e1 */
	$e1 = [];
	array_unshift($e, ...$e1);
	assertType('list<bool|string>', $e);
}

function arrayUnshiftConstantArray(): void
{
	$a = ['foo' => 17, 'a', 'bar' => 18,];
	array_unshift($a, ...[19, 'baz', false]);
	assertType('array{0: 19, 1: \'baz\', 2: false, foo: 17, 3: \'a\', bar: 18}', $a);

	$b = ['foo' => 17, 'a', 'bar' => 18];
	array_unshift($b, 19, 'baz', false);
	assertType('array{0: 19, 1: \'baz\', 2: false, foo: 17, 3: \'a\', bar: 18}', $b);

	$c = ['foo' => 17, 'a', 'bar' => 18];
	array_unshift($c, ...[]);
	assertType('array{foo: 17, 0: \'a\', bar: 18}', $c);

	$d = [];
	array_unshift($d, ...[]);
	assertType('array{}', $d);

	$e = [];
	array_unshift($e, 19, 'baz', false);
	assertType('array{19, \'baz\', false}', $e);

	$f = [17];
	/** @var array<bool|null> $f1 */
	$f1 = [];
	array_unshift($f, ...$f1);
	assertType('non-empty-list<17|bool|null>', $f);

	$g = [new stdClass()];
	array_unshift($g, ...[new stdClass(), new stdClass()]);
	assertType('array{stdClass, stdClass, stdClass}', $g);
}
