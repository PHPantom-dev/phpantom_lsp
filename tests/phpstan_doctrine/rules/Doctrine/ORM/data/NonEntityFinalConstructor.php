<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

class NonEntityFinalConstructor
{
	final public function __construct(string $x)
	{}
}
