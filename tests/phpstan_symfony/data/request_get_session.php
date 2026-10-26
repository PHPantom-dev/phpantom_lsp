<?php declare(strict_types = 1);

use Symfony\Component\HttpFoundation\Session\SessionInterface;
use function PHPStan\Testing\assertType;

/** @var \Symfony\Component\HttpFoundation\Request $request */
$request = doRequest();

$session1 = $request->getSession();
assertType(SessionInterface::class, $request->getSession());

if ($request->hasSession()) {
	assertType(SessionInterface::class, $request->getSession());
}
