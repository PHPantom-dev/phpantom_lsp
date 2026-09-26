<?php

namespace Bug5758;

use function PHPStan\Testing\assertType;

/**
 * @param array{'type':'a','a-param':string}|array{'type':'b','b-param':string} $theInput
 * @return string
 */
function test( array $theInput ) : string
{
	if ( $theInput['type'] === 'a' )
	{
		assertType("array{type: 'a', a-param: string}", $theInput); // SKIP: a union of array shapes is not narrowed by comparing its tag key
		return $theInput['a-param'];
	}
	else
	{
		assertType("array{type: 'b', b-param: string}", $theInput); // SKIP: a union of array shapes is not narrowed by comparing its tag key
		return $theInput['b-param'];
	}
}
