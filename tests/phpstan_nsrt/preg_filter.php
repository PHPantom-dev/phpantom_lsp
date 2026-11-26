<?php

namespace PregFilter;


use function PHPStan\Testing\assertType;

class Foo {
	function doFoo() {
		$subject = array('1', 'a', '2', 'b', '3', 'A', 'B', '4');
		$pattern = array('/\d/', '/[a-z]/', '/[1a]/');
		$replace = array('A:$0', 'B:$0', 'C:$0');

		assertType('array{0?: string, 1?: string, 2?: string, 3?: string, 4?: string, 5?: string, 6?: string, 7?: string}', preg_filter($pattern, $replace, $subject)); // PHPStan says `array<int, string>`; the subject's keys are kept, each one optional
	}

	function doFoo1() {
		$subject = array('1', 'a', '2', 'b', '3', 'A', 'B', '4');
		assertType('array{0?: string, 1?: string, 2?: string, 3?: string, 4?: string, 5?: string, 6?: string, 7?: string}', preg_filter('/\d/', '$0', $subject)); // PHPStan says `array<int, string>`; the subject's keys are kept, each one optional

		$subject = 'hallo';
		assertType('string|null', preg_filter('/\d/', '$0', $subject));
	}

	function doFoo2() {
		$subject = 123;
		assertType('string|null', preg_filter('/\d/', '$0', $subject));

		$subject = 123.123;
		assertType('string|null', preg_filter('/\d/', '$0', $subject));
	}

	public function dooFoo3(string $pattern, string $replace) {
		assertType('string|null', preg_filter($pattern, $replace)); // PHPStan says `list<string>|string|null`; a call without a subject is an ArgumentCountError
		assertType('string|null', preg_filter($pattern)); // PHPStan says `list<string>|string|null`; a call without a subject is an ArgumentCountError
		assertType('string|null', preg_filter()); // PHPStan says `list<string>|string|null`; a call without a subject is an ArgumentCountError
	}

	function bug664() {
		assertType('string|null', preg_filter(['#foo#'], ['bar'], 'subject'));

		assertType('array{0?: string}', preg_filter(['#foo#'], ['bar'], ['subject'])); // PHPStan says `array<int, string>`; the subject's keys are kept, each one optional
	}
}
