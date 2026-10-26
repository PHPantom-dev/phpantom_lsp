<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithBrokenManyToOneRelations
{

	/**
	 * @var int
	 */
	#[ORM\Id]
	#[ORM\Column(type: 'integer')]
	private $id;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity|null
	 */
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToOneNullableBoth;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity|null
	 */
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	#[ORM\JoinColumn(nullable: false)]
	private $manyToOneNullableProperty;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity
	 */
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToOneNullableColumn;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity
	 */
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	#[ORM\JoinColumn(nullable: false)]
	private $manyToOneNonNullable;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\MyEntity|null
	 */
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToOneWrongClass;

}
