<?php

namespace CoalesceAssign;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(
		string $string,
		?string $nullableString
	)
	{
		$emptyArray = [];
		$arrayWithFoo = ['foo' => 'foo'];
		$arrayWithMaybeFoo = [];
		if (rand(0, 1)) {
			$arrayWithMaybeFoo['foo'] = 'foo';
		}

		$arrayAfterAssignment = [];
		$arrayAfterAssignment['foo'] ??= 'foo';

		$arrayWithFooAfterAssignment = ['foo' => 'foo'];
		$arrayWithFooAfterAssignment['foo'] ??= 'bar';

		$nonexistentVariableAfterAssignment ??= 'foo';

		if (rand(0, 1)) {
			$maybeNonexistentVariableAfterAssignment = 'foo';
		}

		$maybeNonexistentVariableAfterAssignment ??= 'bar';

		assertType('string', $string ??= 1); // SKIP: ??= on a target that is set and never null still adds the fallback
		assertType('1|string', $nullableString ??= 1);
		assertType('\'foo\'', $emptyArray['foo'] ??= 'foo');
		assertType('\'foo\'', $arrayWithFoo['foo'] ??= 'bar'); // SKIP: ??= on a target that is set and never null still adds the fallback
		assertType('\'bar\'|\'foo\'', $arrayWithMaybeFoo['foo'] ??= 'bar');
		assertType('array{foo: \'foo\'}', $arrayAfterAssignment);
		assertType('array{foo: \'foo\'}', $arrayWithFooAfterAssignment); // SKIP: ??= on a target that is set and never null still adds the fallback
		assertType('\'foo\'', $nonexistentVariableAfterAssignment);
		assertType('\'bar\'|\'foo\'', $maybeNonexistentVariableAfterAssignment);
	}

}
