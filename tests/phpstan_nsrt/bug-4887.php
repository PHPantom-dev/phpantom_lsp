<?php

namespace Bug4887;

use function PHPStan\Testing\assertType;

assertType('bool', isset($_SESSION));

$foo = ['$_REQUEST' => $_REQUEST,
	'$_COOKIE'  => $_COOKIE,
	'$_SERVER'  => $_SERVER,
	'$GLOBALS'  => $GLOBALS,
	'$SESSION'  => isset($_SESSION) ? $_SESSION : NULL];

foreach ($foo as $data)
{
	assertType('bool', is_array($data));
}
