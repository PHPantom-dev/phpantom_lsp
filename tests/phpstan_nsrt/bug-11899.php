<?php

namespace Bug11899;

use function PHPStan\Testing\assertType;

abstract class Test {}

interface InvertedQuestions {}

class SomeTest extends Test implements InvertedQuestions {}

/**
 * @template TTestType = Test
 * @property-read ?TTestType $test
 */
class UserTest {
	public function __get() : mixed {
		return new SomeTest();
	}
}

function acceptUserTest1(UserTest $ut) : void {
	assertType('Bug11899\\UserTest', $ut);
	assertType('Bug11899\\Test|null', $ut->test);
}

/**
 * @param UserTest<InvertedQuestions> $ut
 */
function acceptUserTest2(UserTest $ut) : void {
	assertType('Bug11899\\UserTest<Bug11899\\InvertedQuestions>', $ut);
	assertType('Bug11899\\InvertedQuestions|null', $ut->test);
}
