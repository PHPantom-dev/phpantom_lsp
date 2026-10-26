<?php // lint >= 8.1

namespace PHPStan\Rules\Doctrine\ORMAttributes;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class FooWithoutPK
{

	#[ORM\Column(type: "string", enumType: FooEnum::class)]
	public FooEnum $type;

}
