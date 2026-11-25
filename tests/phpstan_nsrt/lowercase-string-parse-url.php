<?php

namespace LowercaseStringParseUrl;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param lowercase-string $lowercase
	 */
	public function doParseUrl(string $lowercase): void
	{
		assertType('array{scheme?: lowercase-string, host?: lowercase-string, port?: int<0, 65535>, user?: lowercase-string, pass?: lowercase-string, path?: lowercase-string, query?: lowercase-string, fragment?: lowercase-string}|false', parse_url($lowercase)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_SCHEME)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_HOST)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('int<0, 65535>|false|null', parse_url($lowercase, PHP_URL_PORT)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_USER)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_PASS)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_PATH)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_QUERY)); // SKIP: parse_url() does not carry lowercase-string through to its components
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_FRAGMENT)); // SKIP: parse_url() does not carry lowercase-string through to its components
	}

}
