<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityNonFinalConstructor
{
	public function __construct()
	{}

	final public function foo()
	{}
}
