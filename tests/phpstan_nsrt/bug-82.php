<?php

namespace Bug82;

use PHPStan\TrinaryLogic;
use function PHPStan\Testing\assertType;
use function PHPStan\Testing\assertVariableCertainty;

class Response
{

	function test()
	{
		for ($retryCount = 0; $retryCount <= 64; ++$retryCount) {
			$response = new Response();
			break;
		}

		assertType(self::class, $response);
		assertVariableCertainty(TrinaryLogic::createYes(), $response);
	}

}
