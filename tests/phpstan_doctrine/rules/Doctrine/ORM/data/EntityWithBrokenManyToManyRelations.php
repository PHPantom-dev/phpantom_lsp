<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithBrokenManyToManyRelations
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
	#[ORM\ManyToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToManyWithIterableAnnotation;

	/**
	 * @var \Doctrine\Common\Collections\Collection
	 */
	#[ORM\ManyToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToManyWithCollectionAnnotation;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity[]
	 */
	#[ORM\ManyToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToManyWithArrayAnnotation;

	/**
	 * @var \Doctrine\Common\Collections\Collection&iterable<\PHPStan\Rules\Doctrine\ORM\AnotherEntity>
	 */
	#[ORM\ManyToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToManyWithCorrectAnnotation;

	/**
	 * @var \Doctrine\Common\Collections\Collection|\PHPStan\Rules\Doctrine\ORM\AnotherEntity[]
	 */
	#[ORM\ManyToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToManyWithCorrectOldStyleAnnotation;

}
