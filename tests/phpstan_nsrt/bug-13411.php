<?php declare(strict_types = 1);

namespace Bug13411;

use SplFixedArray;
use function PHPStan\Testing\assertType;

class Token {}

/** @extends SplFixedArray<Token> */
class Tokens extends SplFixedArray {}

/** @param array<int, Token>|Tokens $tokens */
function x(iterable $tokens): int {
	assertType('array<int, Bug13411\\Token>|Bug13411\\Tokens', $tokens);
	return count($tokens);
}
