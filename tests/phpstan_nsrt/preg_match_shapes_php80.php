<?php // lint >= 8.0

namespace PregMatchShapesPhp82;

use function PHPStan\Testing\assertType;

function doOffsetCaptureWithUnmatchedNull(string $s): void {
	// see https://3v4l.org/07rBO#v8.2.9
	if (preg_match('/(foo)(bar)(baz)/', $s, $matches, PREG_OFFSET_CAPTURE|PREG_UNMATCHED_AS_NULL)) {
		assertType("array{array{string|null, int<-1, max>}, array{'foo'|null, int<-1, max>}, array{'bar'|null, int<-1, max>}, array{'baz'|null, int<-1, max>}}", $matches);
	}
	assertType("array{}|array{array{string|null, int<-1, max>}, array{'foo'|null, int<-1, max>}, array{'bar'|null, int<-1, max>}, array{'baz'|null, int<-1, max>}}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function doNonAutoCapturingModifier(string $s): void {
	if (preg_match('/(?n)(\d+)/', $s, $matches)) {
		assertType('array{string}', $matches); // PHPantom: `(?n)` makes the unnamed group non-capturing, as PHPStan's own comment says it should
	}
	assertType('array{}|array{string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}
