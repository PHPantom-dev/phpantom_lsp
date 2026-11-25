<?php // lint >= 8.4

namespace CurlGetinfo84;

use CurlHandle;
use function PHPStan\Testing\assertType;

class Foo
{
	public function bar()
	{
		$handle = new CurlHandle();
		assertType('int|false', curl_getinfo($handle, CURLINFO_POSTTRANSFER_TIME_T));
	}
}
