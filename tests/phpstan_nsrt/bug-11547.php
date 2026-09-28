<?php

namespace Bug11547;

use function PHPStan\Testing\assertType;

function foo(string $s)
{
	$r = preg_replace('/^a/', 'b', $s);
	assertType('string|null', $r);
	return $r;
}

function foobar(string $s, string $pattern)
{
	$r = preg_replace($pattern, 'b', $s);
	assertType('string|null', $r);
	return $r;
}

/**
 * @param array{a: string, b:string} $arr
 */
function bar(array $arr): array
{
	$r = preg_replace('/^a/', 'x', $arr);
	assertType('array{a?: string, b?: string}', $r);
	return $r;
}

/**
 * @param array{a: string, b:string} $arr
 */
function barbar($arr, string $pattern)
{
	$r = preg_replace($pattern, 'b', $arr);
	assertType('array{a?: string, b?: string}', $r);
	return $r;
}

// see https://github.com/phpstan/phpstan/issues/11547#issuecomment-2307156443
// see https://3v4l.org/c70bG
function validPatternWithEmptyResult(string $s, array $arr) {
	$r = preg_replace('/(\D+)*[12]/', 'x', $s);
	assertType('string|null', $r);

	$r = preg_replace('/(\D+)*[12]/', 'x', $arr);
	assertType('array<string>', $r);
}


/**
 * @return string
 */
function fooCallback(string $s)
{
	$r = preg_replace_callback('/^a/', function ($matches) {
		return strtolower($matches[0]);
	}, $s);
	assertType('string|null', $r);
	return $r;
}
