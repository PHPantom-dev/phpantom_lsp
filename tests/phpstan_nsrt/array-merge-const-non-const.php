<?php

namespace ArrayMergeConstNonConst;

use function PHPStan\Testing\assertType;

function doFoo(array $post): void {
}

function doBar(array $array): void {
}

function doFooBar(array $array): void {
}

function doFooInts(array $array): void {
	// int keys will be renumbered therefore we can't reason about them in case we don't know all arrays involved
}

/**
 * @param array<string> $array
 */
function floatKey(array $array): void {
}

function doOptKeys(array $array, array $arr2): void {
	if (rand(0, 1)) {
		$array['abc'] = 'def';
	}
	assertType("array", array_merge($arr2, $array));
	assertType("array", array_merge($array, $arr2));
}

/**
 * @param array{a?: 1, b: 2} $array
 */
function doOptShapeKeys(array $array, array $arr2): void {
}

function hasOffsetKeys(array $array, array $arr2): void {
	if (array_key_exists('b', $array)) {
	}
}

function maybeHasOffsetKeys(array $array): void {
	$arr2 = [];
	if (rand(0,1)) {
		$arr2 ['ab'] = 'def';
	}

	assertType("array", array_merge($arr2, $array));
	assertType("array", array_merge($array, $arr2));
}

function hasOffsetValueKeys(array $hasB, array $mixedArray, array $hasC): void {
	$hasB['b'] = 123;
	$hasC['c'] = 'def';




	if (rand(0, 1)) {
		$hasBorC = ['b' => 1];
	} else {
		$hasBorC = ['c' => 2];
	}
	assertType('array{b: 1}|array{c: 2}', $hasBorC); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (rand(0, 1)) {
		$differentCs = ['c' => 10];
	} else {
		$differentCs = ['c' => 20];
	}
	assertType('array{c: 10|20}', $differentCs); // PHPStan spells the same type as a union of shapes

}

/**
 * @param array{a?: 1, b?: 2} $allOptional
 */
function doAllOptional(array $allOptional, array $arr2): void {
	assertType("array", array_merge($arr2, $allOptional));
	assertType("array", array_merge($allOptional, $arr2));
}
