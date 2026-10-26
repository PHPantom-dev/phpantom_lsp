<?php declare(strict_types = 1);

use function PHPStan\Testing\assertType;

/** @var \Symfony\Component\HttpFoundation\Request $request */
$request = doFoo();

assertType('string', $request->getContent());
assertType('string', $request->getContent(false));
assertType('resource', $request->getContent(true));
assertType('resource|string', $request->getContent(doBar()));
