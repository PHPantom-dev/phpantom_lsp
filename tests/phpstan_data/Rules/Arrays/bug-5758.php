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
		assertType("array{type: 'a', a-param: string}", $theInput);
		return $theInput['a-param'];
	}
	else
	{
		assertType("array{type: 'b', b-param: string}", $theInput);
		return $theInput['b-param'];
	}
}
