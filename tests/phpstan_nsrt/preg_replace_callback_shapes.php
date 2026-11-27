<?php

namespace PregReplaceCallbackMatchShapes;

use function PHPStan\Testing\assertType;

function (string $s): void {
	preg_replace_callback(
		'/(foo)?(bar)?(baz)?/',
		function ($matches) {
			assertType("array{string, 'foo'|null, 'bar'|null, 'baz'|null}", $matches);
			return '';
		},
		$s,
		-1,
		$count,
		PREG_UNMATCHED_AS_NULL
	);
};

function (string $s): void {
	preg_replace_callback(
		'/(foo)?(bar)?(baz)?/',
		function ($matches) {
			assertType("list{0: array{string, int<-1, max>}, 1?: array{''|'foo', int<-1, max>}, 2?: array{''|'bar', int<-1, max>}, 3?: array{'baz', int<-1, max>}}", $matches);
			return '';
		},
		$s,
		-1,
		$count,
		PREG_OFFSET_CAPTURE
	);
};

function (string $s): void {
	preg_replace_callback(
		'/(foo)?(bar)?(baz)?/',
		function ($matches) {
			assertType("array{array{string|null, int<-1, max>}, array{'foo'|null, int<-1, max>}, array{'bar'|null, int<-1, max>}, array{'baz'|null, int<-1, max>}}", $matches);
			return '';
		},
		$s,
		-1,
		$count,
		PREG_OFFSET_CAPTURE|PREG_UNMATCHED_AS_NULL
	);
};

function bug12792(string $string) : void {
	preg_replace_callback(
		'~\'(?:[^\']+|\'\')*+\'\K|\[(\w*)\]~',
		function ($matches) {
			assertType("array{0: string, 1?: string}", $matches);
			return '';
		},
		$string
	);
}
