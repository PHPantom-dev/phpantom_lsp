<?php declare(strict_types=1);

namespace Bug3312;

use function PHPStan\Testing\assertType;

function sayHello(): void
{
	$arr = ['one' => 'een', 'two' => 'twee', 'three' => 'drie'];
	usort($arr, 'strcmp');
	assertType("non-empty-list<'drie'|'een'|'twee'>", $arr);
}
