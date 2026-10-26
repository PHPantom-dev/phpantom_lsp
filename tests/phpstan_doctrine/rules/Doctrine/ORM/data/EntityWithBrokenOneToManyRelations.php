<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithBrokenOneToManyRelations
{

	/**
	 * @var int
	 */
	#[ORM\Id]
	#[ORM\Column(type: 'integer')]
	private $id;

	/**
	 * @var iterable<\PHPStan\Rules\Doctrine\ORM\AnotherEntity>
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'one')]
	private $oneToManyWithIterableAnnotation;

	/**
	 * @var \Doctrine\Common\Collections\Collection
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'two')]
	private $oneToManyWithCollectionAnnotation;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity[]
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'three')]
	private $oneToManyWithArrayAnnotation;

	/**
	 * @var \Doctrine\Common\Collections\Collection&iterable<\PHPStan\Rules\Doctrine\ORM\AnotherEntity>
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'four')]
	private $oneToManyWithCorrectAnnotation;

	/**
	 * @var \Doctrine\Common\Collections\Collection|\PHPStan\Rules\Doctrine\ORM\AnotherEntity[]
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'five')]
	private $oneToManyWithCorrectOldStyleAnnotation;

}
