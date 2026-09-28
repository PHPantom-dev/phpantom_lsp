<?php

namespace Bug3474;

use function PHPStan\Testing\assertType;

/**
 * @param mixed[] $a
 */
function acceptsArray(array $a) : void{

}

/**
 * @param mixed[] $a
 */
function dummy(array $a) : void{
	if(!is_array($a["test"])){
		throw new \RuntimeException("oops");
	}

	assertType('array<mixed, mixed>', $a['test']);
	assertType('array<mixed, mixed>', $a["test"]);

	acceptsArray($a['test']);
}
