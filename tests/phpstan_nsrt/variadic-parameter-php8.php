<?php // lint >= 8.0

namespace VariadicParameterPHP8;

use function PHPStan\Testing\assertType;

function foo(...$args)
{
	assertType('array<int|string, mixed>', $args);
	assertType('mixed', $args['foo']);
	assertType('mixed', $args['bar']);
}

function bar(string ...$args)
{
	assertType('array<int|string, string>', $args);
}

