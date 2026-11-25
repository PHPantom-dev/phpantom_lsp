<?php

namespace Bug4950;

use function PHPStan\Testing\assertType;

class TestType
{
	public const TEST_1 = 'test1';
	public const TEST_2 = 'test2';
	public const TEST_3 = 'test3';

	public const ALL = [
		self::TEST_1,
		self::TEST_2,
		self::TEST_3,
	];
}

class Another
{
	public const TEST_TYPES = [
		...TestType::ALL,
		'test4',
		'test5',
	];

	function test(): void {
		assertType("array{'test1', 'test2', 'test3'}", TestType::ALL);
		assertType("array{'test1', 'test2', 'test3', 'test4', 'test5'}", self::TEST_TYPES);
		foreach (self::TEST_TYPES as $testType) {
			assertType("'test1'|'test2'|'test3'|'test4'|'test5'", $testType);
		}
	}
}
