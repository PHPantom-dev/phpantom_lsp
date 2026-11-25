<?php declare(strict_types = 1);

namespace TemplateConstantBound;

use function PHPStan\Testing\assertType;

/**
 * @template T1 of 'foo'
 * @template T2 of 5
 * @param T1 $foo
 * @param T2 $bar
 */
function foo(string $foo, int $bar): void
{
	assertType("T1 of 'foo' (function TemplateConstantBound\\foo(), argument)", $foo);
	assertType('T2 of 5 (function TemplateConstantBound\foo(), argument)', $bar);
}
