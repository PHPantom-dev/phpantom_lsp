<?php

namespace Bug4754;

use function PHPStan\Testing\assertType;

/**
 * The standard PHP parse_url() function doesn't work with relative URI's only absolute URL's
 * so this function works around that by adding a http://domain prefix
 * if needed and passing through the results
 *
 * @param string $url
 * @param int	 $component
 *
 * @return string|int|array|bool|null
 */
function parseUrl($url, $component = -1)
{
	$parsedComponentNotSpecified = parse_url($url);
	$parsedNotConstant = parse_url($url, $component);
	$parsedAllConstant = parse_url($url, -1);
	$parsedSchemeConstant = parse_url($url, PHP_URL_SCHEME);
	$parsedHostConstant = parse_url($url, PHP_URL_HOST);
	$parsedPortConstant = parse_url($url, PHP_URL_PORT);
	$parsedUserConstant = parse_url($url, PHP_URL_USER);
	$parsedPassConstant = parse_url($url, PHP_URL_PASS);
	$parsedPathConstant = parse_url($url, PHP_URL_PATH);
	$parsedQueryConstant = parse_url($url, PHP_URL_QUERY);
	$parsedFragmentConstant = parse_url($url, PHP_URL_FRAGMENT);

	assertType('array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|false', $parsedComponentNotSpecified);
	assertType('array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|int<0, 65535>|string|false|null', $parsedNotConstant);
	assertType('array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|false', $parsedAllConstant);
	assertType('string|false|null', $parsedSchemeConstant);
	assertType('string|false|null', $parsedHostConstant);
	assertType('int<0, 65535>|false|null', $parsedPortConstant);
	assertType('string|false|null', $parsedUserConstant);
	assertType('string|false|null', $parsedPassConstant);
	assertType('string|false|null', $parsedPathConstant);
	assertType('string|false|null', $parsedQueryConstant);
	assertType('string|false|null', $parsedFragmentConstant);
}
