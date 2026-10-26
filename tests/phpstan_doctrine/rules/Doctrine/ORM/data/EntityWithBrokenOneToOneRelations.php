<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithBrokenOneToOneRelations
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
	#[ORM\OneToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $oneToOneNullableBoth;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity|null
	 */
	#[ORM\OneToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	#[ORM\JoinColumn(nullable: false)]
	private $oneToOneNullableProperty;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity
	 */
	#[ORM\OneToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $oneToOneNullableColumn;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity
	 */
	#[ORM\OneToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	#[ORM\JoinColumn(nullable: false)]
	private $oneToOneNonNullable;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\MyEntity|null
	 */
	#[ORM\OneToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $oneToOneWrongClass;

}
