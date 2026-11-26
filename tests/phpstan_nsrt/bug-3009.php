<?php

namespace Bug3009;

use function PHPStan\Testing\assertType;

class HelloWorld
{

	public function createRedirectRequest(string $redirectUri): ?string
	{
		$redirectUrlParts = parse_url($redirectUri);
		if (false === is_array($redirectUrlParts) || true === array_key_exists('host', $redirectUrlParts)) {
			assertType('array{scheme?: string, host: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|false', $redirectUrlParts);
			return null;
		}

		assertType('array{scheme?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}', $redirectUrlParts);

		if (true === array_key_exists('query', $redirectUrlParts)) {
			assertType('array{scheme?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query: string, fragment?: string}', $redirectUrlParts);
			$redirectServer['QUERY_STRING'] = $redirectUrlParts['query'];
		}

		assertType('array{scheme?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}', $redirectUrlParts);

		return 'foo';
	}

}
