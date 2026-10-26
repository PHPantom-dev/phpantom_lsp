<?php // lint >= 8.1

namespace MissingReadOnlyPropertyAssign;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithAGeneratedId
{

	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column]
	private readonly int $id; // ok, ID is generated

	#[ORM\Column]
	private readonly int $unassigned;

	#[ORM\Column]
	private readonly int $doubleAssigned;

	public function __construct(int $doubleAssigned)
	{
		$this->doubleAssigned = $doubleAssigned;
		$this->doubleAssigned = 17;
	}

}

#[ORM\Entity(null, true)]
class ReadOnlyEntity
{

	#[ORM\Id]
	#[ORM\Column]
	private readonly int $id; // ok, entity is readonly

}

#[ORM\Entity(null, true)]
class ReadOnlyEntityWithConstructor
{

	#[ORM\Id]
	#[ORM\Column]
	private readonly int $id;

	public function __construct()
	{
	}

}
