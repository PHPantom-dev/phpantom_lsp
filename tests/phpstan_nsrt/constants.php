<?php

namespace ConstantsForNodeScopeResolverTest;

use function PHPStan\Testing\assertType;

const FOO_CONSTANT = 1;

$foo = FOO_CONSTANT;

define('BAR_CONSTANT', 'bar');

if (defined('BAZ_CONSTANT')) {
	assertType('1', $foo);
	assertType('*ERROR*', NONEXISTENT_CONSTANT);
	assertType('\'bar\'', \BAR_CONSTANT);
	assertType('mixed', \BAZ_CONSTANT);
}
