<?php // lint >= 8.1

namespace Constant;

use function PHPStan\Testing\assertType;

define('FOO', 'foo');
const BAR = 'bar';

class Baz
{
	const BAZ = 'baz';
}

enum Suit
{
    case Hearts;
}

function doFoo(string $constantName): void
{
	assertType('mixed', constant($constantName));
}

assertType("'foo'", FOO);
assertType("'foo'", constant('FOO'));
assertType("*ERROR*", constant('\Constant\FOO'));

assertType("'bar'", BAR);
assertType("*ERROR*", constant('BAR'));
assertType("'bar'", constant('\Constant\BAR'));

assertType("'bar'|'foo'", constant(rand(0, 1) ? 'FOO' : '\Constant\BAR'));

assertType("'baz'", constant('\Constant\Baz::BAZ'));

assertType('Constant\Suit::Hearts', Suit::Hearts);
assertType('Constant\Suit::Hearts', constant('\Constant\Suit::Hearts'));

assertType('*ERROR*', constant('UNDEFINED'));
assertType('*ERROR*', constant('::aa'));
