<?php // lint >= 8.2

namespace CurlGetinfo82;

use CurlHandle;
use function PHPStan\Testing\assertType;

class Foo
{
	public function bar()
	{
		$handle = new CurlHandle();
		assertType('string', curl_getinfo($handle, CURLINFO_EFFECTIVE_METHOD));
		assertType('int', curl_getinfo($handle, CURLINFO_PROXY_ERROR));
		assertType('string|false', curl_getinfo($handle, CURLINFO_REFERER));
		assertType('int', curl_getinfo($handle, CURLINFO_RETRY_AFTER));
	}
}
