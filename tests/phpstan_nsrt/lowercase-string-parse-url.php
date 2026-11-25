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
		assertType('array{scheme?: lowercase-string, host?: lowercase-string, port?: int<0, 65535>, user?: lowercase-string, pass?: lowercase-string, path?: lowercase-string, query?: lowercase-string, fragment?: lowercase-string}|false', parse_url($lowercase));
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_SCHEME));
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_HOST));
		assertType('int<0, 65535>|false|null', parse_url($lowercase, PHP_URL_PORT));
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_USER));
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_PASS));
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_PATH));
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_QUERY));
		assertType('lowercase-string|false|null', parse_url($lowercase, PHP_URL_FRAGMENT));
	}

}
