<?php

namespace SpecifiedTypesClosureUse;

use PhpParser\Node\Expr\MethodCall;
use PhpParser\Node\Identifier;
use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(MethodCall $call, MethodCall $bar): void
	{
		if ($call->name instanceof Identifier && $bar->name instanceof Identifier) {
			function () use ($call): void {
				assertType('PhpParser\Node\Identifier', $call->name);
				assertType('mixed', $bar->name);
			};

			assertType('PhpParser\Node\Identifier', $call->name);
		}
	}

	public function doBar(MethodCall $call, MethodCall $bar): void
	{
		if ($call->name instanceof Identifier && $bar->name instanceof Identifier) {
			$a = 1;
			function () use ($call, &$a): void {
				assertType('PhpParser\Node\Identifier', $call->name);
				assertType('mixed', $bar->name);
			};

			assertType('PhpParser\Node\Identifier', $call->name);
		}
	}

	public function doBaz(array $arr, string $key): void
	{
		$arr[$key] = 'test';
		assertType('non-empty-array', $arr);
		assertType("'test'", $arr[$key]);
		function ($arr) use ($key): void {
			assertType('string', $key);
			assertType('mixed', $arr);
			assertType('mixed', $arr[$key]);
		};
	}
	public function doBuzz(array $arr, string $key): void
	{
		if (isset($arr[$key])) {
			function () use ($arr, $key): void {
			};
		}
	}

	public function doBuzz(array $arr, string $key): void
	{
		if (isset($arr[$key])) {
			function ($key) use ($arr): void {
				assertType("mixed", $arr[$key]);
			};
		}
	}

}
