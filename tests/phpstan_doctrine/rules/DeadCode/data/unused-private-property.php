<?php // lint >= 7.4

namespace PHPStan\Rules\Doctrine\ORM\UnusedPrivateProperty;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithAGeneratedId
{

	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column]
	private int $id; // ok, ID is generated

	#[ORM\Column]
	private int $unused;

	private int $unused2;

}

#[ORM\Entity(readOnly: true)]
class ReadOnlyEntity
{

	#[ORM\Id]
	#[ORM\Column]
	private int $id; // ok, entity is read only

}

#[ORM\Entity(readOnly: true)]
class ReadOnlyEntityWithConstructor
{

	#[ORM\Id]
	#[ORM\Column]
	private int $id;

	public function __construct()
	{
	}
}

#[ORM\Entity]
class EntityWithGeneratedField
{
	#[ORM\Id]
	#[ORM\Column]
	public int $id;

	#[ORM\Column(type: 'int', insertable: false, updatable: false, generated: 'ALWAYS', columnDefinition: 'int GENERATED ALWAYS AS (1 + 2)')]
	private int $generated;

	public function __construct()
	{
	}
}

#[ORM\Entity]
class EntityWithGeneratedFieldWithGetter
{
	#[ORM\Id]
	#[ORM\Column]
	public int $id;

	#[ORM\Column(type: 'int', insertable: false, updatable: false, generated: 'ALWAYS', columnDefinition: 'int GENERATED ALWAYS AS (1 + 2)')]
	private int $generated;

	public function __construct()
	{
	}

	/**
	 * @return int
	 */
	public function getGenerated(): int
	{
		return $this->generated;
	}
}

