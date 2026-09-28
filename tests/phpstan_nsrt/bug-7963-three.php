<?php

namespace Bug7963Three;

use function PHPStan\Testing\assertType;

function (): void {
	/**
	 * @var array{
	 *     foo: string|null,
	 *     bar: string|null,
	 *     baz: string|null,
	 *     abc: string|null,
	 *     qwe: string|null,
	 *     xyz: string|null,
	 * } $arr
	 */
	$arr = [];

	$str = 'Info: ';
	$str .= $arr['foo'] !== null ? "[foo:$arr[foo]]" : '[no-foo]';
	$str .= $arr['bar'] !== null ? "[bar:$arr[bar]]" : '[no-bar]';
	$str .= $arr['baz'] !== null ? "[baz:$arr[baz]]" : '[no-baz]';
	assertType('array{foo: string|null, bar: string|null, baz: string|null, abc: string|null, qwe: string|null, xyz: string|null}', $arr);
	$str .= $arr['abc'] !== null ? "[abc:$arr[abc]]" : '[no-abc]';
	$str .= $arr['qwe'] !== null ? "[qwe:$arr[qwe]]" : '[no-qwe]';
	$str .= $arr['xyz'] !== null ? "[xyz:$arr[xyz]]" : '[no-xyz]';
	assertType('array{foo: string|null, bar: string|null, baz: string|null, abc: string|null, qwe: string|null, xyz: string|null}', $arr);
};
