<?php

namespace Bug3997Type;

use function PHPStan\Testing\assertType;

function (\Countable $c): void {
	assertType('int<0, max>', $c->count());
	assertType('int<0, max>', count($c));
};

function (\ArrayIterator $i): void {
	assertType('int<0, max>', $i->count());
	assertType('int<0, max>', count($i));
};
