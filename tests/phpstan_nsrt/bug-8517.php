<?php declare(strict_types = 1);

namespace Bug8517;

use function PHPStan\Testing\assertType;

function test(?\stdClass $request): void
{
	assertType('stdClass|null', $request);
	$routeParams0 = $request?->attributes->get('_route_params', []);
	assertType('stdClass|null', $request);
	$routeParams = $request?->attributes->get('_route_params', []) ?? [];
	$param = $request?->attributes->get('_param') ?? $routeParams['_param'];
	assertType('stdClass|null', $request);
}
