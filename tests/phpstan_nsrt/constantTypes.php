<?php

namespace ConstantTypesIncrementDecrement;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var int */
	private $intProperty;

	/** @var int */
	private static $staticIntProperty;

	/** @var int */
	private $anotherIntProperty;

	/** @var int */
	private static $anotherStaticIntProperty;

	public function doFoo()
	{
		$postIncrement = 1;
		$postIncrement++;

		$preIncrement = 1;
		++$preIncrement;

		$postDecrement = 5;
		$postDecrement--;

		$preDecrement = 5;
		--$preDecrement;

		$literalArray = [
			'a' => 1,
			'b' => 5,
			'c' => 1,
			'd' => 5,
		];
		$literalArray['a']++;
		$literalArray['b']--;
		++$literalArray['c'];
		--$literalArray['d'];

		$nullIncremented = null;
		$nullDecremented = null;
		$nullIncremented++;
		$nullDecremented--;

		$incrementInIf = 1;
		$anotherIncrementInIf = 1;
		$valueOverwrittenInIf = 1;
		$anotherValueOverwrittenInIf = 10;
		$appendingToArrayInBranches = [];

		$this->anotherIntProperty = 1;
		self::$anotherStaticIntProperty = 1;
		if (doFoo()) {
			$incrementInIf++;
			$anotherIncrementInIf++;
			$valueOverwrittenInIf = 2;
			$anotherValueOverwrittenInIf /= 2;
			$appendingToArrayInBranches[] = 1;
			$appendingToArrayInBranches[] = 2;
			$this->anotherIntProperty++;
			self::$anotherStaticIntProperty++;
		} elseif (doBar()) {
			$incrementInIf++;
			$incrementInIf++;
			$anotherIncrementInIf++;
			$anotherIncrementInIf++;
		} else {
			$anotherIncrementInIf++;
		}

		$incrementInForLoop = 1;
		$valueOverwrittenInForLoop = 1;
		$arrayOverwrittenInForLoop = [
			'a' => 1,
			'b' => 'foo',
		];

		$this->intProperty = 1;
		self::$staticIntProperty = 1;
		for ($i = 0; $i < 10; $i++) {
			$incrementInForLoop++;
			$valueOverwrittenInForLoop = 2;
			$arrayOverwrittenInForLoop['a']++;
			$arrayOverwrittenInForLoop['b'] = 'bar';
			$this->intProperty++;
			self::$staticIntProperty++;
		}

		$intProperty = $this->intProperty;
		$staticIntProperty = self::$staticIntProperty;
		$anotherIntProperty = $this->anotherIntProperty;
		$anotherStaticIntProperty = self::$anotherStaticIntProperty;

		$variableIncrementedInClosurePassedByReference = 0;
		$anotherVariableIncrementedInClosure = 0;
		$yetAnotherVariableInClosurePassedByReference = 0;
		function () use (&$variableIncrementedInClosurePassedByReference, $anotherVariableIncrementedInClosure, &$yetAnotherVariableInClosurePassedByReference) {
			$variableIncrementedInClosurePassedByReference++;
			$anotherVariableIncrementedInClosure++;
			$yetAnotherVariableInClosurePassedByReference = 1;
		};

		$variableIncrementedInFinally = 0;
		try {
			doFoo();
		} finally {
			$variableIncrementedInFinally++;
		}

		assertType('2', $postIncrement);
		assertType('4', $postDecrement);
		assertType('2', $preIncrement);
		assertType('4', $preDecrement);
		assertType('array{a: 2, b: 4, c: 2, d: 4}', $literalArray); // SKIP: an increment of an array offset or a property is not written back
		assertType('1', $nullIncremented);
		assertType('null', $nullDecremented);
		assertType('1|2|3', $incrementInIf);
		assertType('2|3', $anotherIncrementInIf);
		assertType('1|2', $valueOverwrittenInIf);
		assertType('5|10', $anotherValueOverwrittenInIf);
		assertType('int<2, max>', $incrementInForLoop);
		assertType('2', $valueOverwrittenInForLoop);
		assertType('array{a: int<2, max>, b: \'bar\'}', $arrayOverwrittenInForLoop); // SKIP: an increment of an array offset or a property is not written back
		assertType('int<2, max>', $intProperty); // SKIP: an increment of an array offset or a property is not written back
		assertType('int<2, max>', $staticIntProperty); // SKIP: an increment of an array offset or a property is not written back
		assertType('1|2', $anotherIntProperty); // SKIP: an increment of an array offset or a property is not written back
		assertType('1|2', $anotherStaticIntProperty); // SKIP: an increment of an array offset or a property is not written back
		assertType('int<0, max>', $variableIncrementedInClosurePassedByReference);
		assertType('0', $anotherVariableIncrementedInClosure);
		assertType('0|1', $yetAnotherVariableInClosurePassedByReference);
		assertType('1', $variableIncrementedInFinally);
	}

}
