<?php

namespace ClassReflectionInterfaces;

use function PHPStan\Testing\assertType;

/**
 * @extends \Traversable<int, mixed>
 */
interface ResultStatement extends \Traversable
{

}

interface Statement extends ResultStatement
{

}

function (Statement $s): void
{
	foreach ($s as $k => $v) {
		assertType('int', $k);
		assertType('mixed', $v);
	}
};
