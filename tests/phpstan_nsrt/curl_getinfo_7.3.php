<?php // lint >= 7.3

namespace CurlGetinfo73;

use CurlHandle;
use function PHPStan\Testing\assertType;

class Foo
{
	public function bar()
	{
		$handle = new CurlHandle();
		assertType('int', curl_getinfo($handle, CURLINFO_CONTENT_LENGTH_DOWNLOAD_T));
		assertType('int', curl_getinfo($handle, CURLINFO_CONTENT_LENGTH_UPLOAD_T));
		assertType('int', curl_getinfo($handle, CURLINFO_HTTP_VERSION));
		assertType('int', curl_getinfo($handle, CURLINFO_PROTOCOL));
		assertType('int', curl_getinfo($handle, CURLINFO_PROXY_SSL_VERIFYRESULT));
		assertType('string', curl_getinfo($handle, CURLINFO_SCHEME));
		assertType('int', curl_getinfo($handle, CURLINFO_SIZE_DOWNLOAD_T));
		assertType('int', curl_getinfo($handle, CURLINFO_SIZE_UPLOAD_T));
		assertType('int', curl_getinfo($handle, CURLINFO_SPEED_DOWNLOAD_T));
		assertType('int', curl_getinfo($handle, CURLINFO_SPEED_UPLOAD_T));
		assertType('int', curl_getinfo($handle, CURLINFO_APPCONNECT_TIME_T));
		assertType('int', curl_getinfo($handle, CURLINFO_CONNECT_TIME_T));
		assertType('int', curl_getinfo($handle, CURLINFO_FILETIME_T));
		assertType('int', curl_getinfo($handle, CURLINFO_NAMELOOKUP_TIME_T));
		assertType('int', curl_getinfo($handle, CURLINFO_PRETRANSFER_TIME_T));
		assertType('int', curl_getinfo($handle, CURLINFO_REDIRECT_TIME_T));
		assertType('int', curl_getinfo($handle, CURLINFO_STARTTRANSFER_TIME_T));
		assertType('int', curl_getinfo($handle, CURLINFO_TOTAL_TIME_T));
	}
}
