<?php // lint >= 8.3

declare(strict_types = 1);

namespace StrIncrementFunctionReturn;

use function PHPStan\Testing\assertType;

/**
 * @param non-empty-string $s
 */
function foo(string $s)
{
	assertType('non-falsy-string', str_increment($s));
	assertType('*NEVER*', str_increment(''));
	assertType('*NEVER*', str_increment('='));
	assertType('*NEVER*', str_increment('0.0'));
	assertType('*NEVER*', str_increment('1.0'));
	assertType('*NEVER*', str_increment('字'));
	assertType("'1'", str_increment('0'));
	assertType("'2'", str_increment('1'));
	assertType("'b'", str_increment('a'));
	assertType("'B'", str_increment('A'));
	assertType("'10'", str_increment('9'));
	assertType("'11'", str_increment('10'));
	assertType("'20'", str_increment('19'));
	assertType("'1b'", str_increment('1a'));
	assertType("'1f'", str_increment('1e'));
	assertType("'1g'", str_increment('1f'));
	assertType("'2a'", str_increment('1z'));
	assertType("'10a'", str_increment('9z'));
	assertType("'b'", str_increment('a'));
	assertType("'1f2'", str_increment('1f1'));
	assertType("'z'", str_increment('y'));
	assertType("'aa'", str_increment('z'));
	assertType("'z1'", str_increment('z0'));
	assertType("'aaa'", str_increment('zz'));
}

/**
 * @param 'b'|'1' $s1
 * @param 1|string $s2
 */
function union($s1, $s2): void
{
	assertType("'2'|'c'", str_increment($s1));
	assertType('non-falsy-string', str_increment($s2));
}

/**
 * @param 'b'|'' $s
 */
function unionContainsInvalidInput($s): void
{
	assertType("'c'", str_increment($s));
}
