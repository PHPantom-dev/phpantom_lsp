<?php declare(strict_types=1);

namespace Bug5783;

use function PHPStan\Testing\assertType;

class HelloWorld {}

function foo(): void
{
	$a = [new HelloWorld()];
	assertType('array{Bug5783\HelloWorld}', $a);
	$b = [new HelloWorld(), new HelloWorld(), new HelloWorld()];
	assertType('array{Bug5783\HelloWorld, Bug5783\HelloWorld, Bug5783\HelloWorld}', $b);

	array_push($a, ...$b);
	assertType('array{Bug5783\HelloWorld, Bug5783\HelloWorld, Bug5783\HelloWorld, Bug5783\HelloWorld}', $a);
}
