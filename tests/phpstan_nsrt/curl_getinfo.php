<?php

namespace CurlGetinfo;

use CurlHandle;
use function PHPStan\Testing\assertType;

class Foo
{
	public function bar()
	{
		$curlGetInfoType = '(array{url: string, content_type: string|null, http_code: int, header_size: int, request_size: int, filetime: int, ssl_verify_result: int, redirect_count: int, total_time: float, namelookup_time: float, connect_time: float, pretransfer_time: float, size_upload: float, size_download: float, speed_download: float, speed_upload: float, download_content_length: float, upload_content_length: float, starttransfer_time: float, redirect_time: float, redirect_url: string, primary_ip: string, certinfo: list<array<string, string>>, primary_port: int, local_ip: string, local_port: int, http_version: int, protocol: int, ssl_verifyresult: int, scheme: string, appconnect_time_us: int, queue_time_us: int, connect_time_us: int, namelookup_time_us: int, pretransfer_time_us: int, redirect_time_us: int, starttransfer_time_us: int, posttransfer_time_us: int, total_time_us: int, request_header: string, effective_method: string, capath: string, cainfo: string, used_proxy: int, httpauth_used: int, proxyauth_used: int, conn_id: int}|false)';

		$handle = new CurlHandle();
		assertType('mixed', curl_getinfo());
		assertType('mixed', CURL_GETINFO());
		assertType('mixed', CuRl_GeTiNfO());
		assertType('mixed', curl_getinfo($handle, 'Invalid Argument')); // PHPStan says `false`, but a non-numeric string option is a TypeError
		assertType($curlGetInfoType, curl_getinfo($handle, PHP_INT_MAX));
		assertType('mixed', curl_getinfo($handle, PHP_EOL)); // PHPStan says `false`, but a non-numeric string option is a TypeError
		assertType($curlGetInfoType, curl_getinfo($handle));
		assertType($curlGetInfoType, curl_getinfo($handle, null));
		assertType('string', curl_getinfo($handle, CURLINFO_EFFECTIVE_URL));
		assertType('string', curl_getinfo($handle, 1048577)); // CURLINFO_EFFECTIVE_URL int value without using constant
		assertType('false', curl_getinfo($handle, 12345678)); // Non constant non CURLINFO_* int value
		assertType('int', curl_getinfo($handle, CURLINFO_FILETIME));
		assertType('float', curl_getinfo($handle, CURLINFO_TOTAL_TIME));
		assertType('float', curl_getinfo($handle, CURLINFO_NAMELOOKUP_TIME));
		assertType('float', curl_getinfo($handle, CURLINFO_CONNECT_TIME));
		assertType('float', curl_getinfo($handle, CURLINFO_PRETRANSFER_TIME));
		assertType('float', curl_getinfo($handle, CURLINFO_STARTTRANSFER_TIME));
		assertType('int', curl_getinfo($handle, CURLINFO_REDIRECT_COUNT));
		assertType('float', curl_getinfo($handle, CURLINFO_REDIRECT_TIME));
		assertType('string|false', curl_getinfo($handle, CURLINFO_REDIRECT_URL));
		assertType('string', curl_getinfo($handle, CURLINFO_PRIMARY_IP));
		assertType('int', curl_getinfo($handle, CURLINFO_PRIMARY_PORT));
		assertType('string', curl_getinfo($handle, CURLINFO_LOCAL_IP));
		assertType('int', curl_getinfo($handle, CURLINFO_LOCAL_PORT));
		assertType('float', curl_getinfo($handle, CURLINFO_SIZE_UPLOAD));
		assertType('float', curl_getinfo($handle, CURLINFO_SIZE_DOWNLOAD));
		assertType('float', curl_getinfo($handle, CURLINFO_SPEED_DOWNLOAD));
		assertType('float', curl_getinfo($handle, CURLINFO_SPEED_UPLOAD));
		assertType('int', curl_getinfo($handle, CURLINFO_HEADER_SIZE));
		assertType('string|false', curl_getinfo($handle, CURLINFO_HEADER_OUT));
		assertType('int', curl_getinfo($handle, CURLINFO_REQUEST_SIZE));
		assertType('int', curl_getinfo($handle, CURLINFO_SSL_VERIFYRESULT));
		assertType('float', curl_getinfo($handle, CURLINFO_CONTENT_LENGTH_DOWNLOAD));
		assertType('float', curl_getinfo($handle, CURLINFO_CONTENT_LENGTH_UPLOAD));
		assertType('string|false', curl_getinfo($handle, CURLINFO_CONTENT_TYPE));
		assertType('mixed', curl_getinfo($handle, CURLINFO_PRIVATE));
		assertType('int', curl_getinfo($handle, CURLINFO_RESPONSE_CODE));
		assertType('int', curl_getinfo($handle, CURLINFO_HTTP_CODE));;
		assertType('int', curl_getinfo($handle, CURLINFO_HTTP_CONNECTCODE));
		assertType('int', curl_getinfo($handle, CURLINFO_HTTPAUTH_AVAIL));
		assertType('int', curl_getinfo($handle, CURLINFO_PROXYAUTH_AVAIL));
		assertType('int', curl_getinfo($handle, CURLINFO_OS_ERRNO));
		assertType('int', curl_getinfo($handle, CURLINFO_NUM_CONNECTS));
		assertType('list<string>', curl_getinfo($handle, CURLINFO_SSL_ENGINES));
		assertType('list<string>', curl_getinfo($handle, CURLINFO_COOKIELIST));
		assertType('string|false', curl_getinfo($handle, CURLINFO_FTP_ENTRY_PATH));
		assertType('float', curl_getinfo($handle, CURLINFO_APPCONNECT_TIME));
		assertType('list<array<string, string>>', curl_getinfo($handle, CURLINFO_CERTINFO));
		assertType('int', curl_getinfo($handle, CURLINFO_CONDITION_UNMET));
		assertType('int', curl_getinfo($handle, CURLINFO_RTSP_CLIENT_CSEQ));
		assertType('int', curl_getinfo($handle, CURLINFO_RTSP_CSEQ_RECV));
		assertType('int', curl_getinfo($handle, CURLINFO_RTSP_SERVER_CSEQ));
		assertType('string|false', curl_getinfo($handle, CURLINFO_RTSP_SESSION_ID));
	}
}
