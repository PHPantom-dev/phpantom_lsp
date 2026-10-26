<?php

namespace InputBag;

use Symfony\Component\HttpFoundation\Request;
use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(Request $request): void
	{
		assertType('bool|float|int|string|null', $request->request->get('foo'));
		assertType('string|null', $request->query->get('foo'));
		assertType('string|null', $request->cookies->get('foo'));

		assertType('bool|float|int|string', $request->request->get('foo', 'foo')); // SKIP: Symfony return types an argument decides
		assertType('string', $request->query->get('foo', 'foo'));
		assertType('string', $request->cookies->get('foo', 'foo'));
	}

}
