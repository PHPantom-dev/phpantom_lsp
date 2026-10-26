<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class MyEntityRelationNullablePrimaryKey
{
	/**
	 * @var MyEntity|null
	 */
	#[ORM\Id]
	#[ORM\ManyToOne(targetEntity: MyEntity::class)]
	private $id;

}
