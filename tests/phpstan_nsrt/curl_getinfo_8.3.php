<?php // lint >= 8.3

namespace CurlGetinfo83;

use CurlHandle;
use function PHPStan\Testing\assertType;

class Foo
{
	public function bar()
	{
		$handle = new CurlHandle();
		assertType('string|false', curl_getinfo($handle, CURLINFO_CAINFO));
		assertType('string|false', curl_getinfo($handle, CURLINFO_CAPATH));
	}
}
