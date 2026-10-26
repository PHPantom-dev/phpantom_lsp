<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithRelations
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
	private $oneToOne;

	/**
	 * @var \PHPStan\Rules\Doctrine\ORM\AnotherEntity|null
	 */
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToOne;

	/**
	 * @var \Doctrine\Common\Collections\Collection&iterable<\PHPStan\Rules\Doctrine\ORM\AnotherEntity>
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'manyToOne')]
	private $oneToMany;

	/**
	 * @var \Doctrine\Common\Collections\Collection&iterable<\PHPStan\Rules\Doctrine\ORM\AnotherEntity>
	 */
	#[ORM\ManyToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $manyToMany;

	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $mixed;

	/**
	 * @var int&string
	 */
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity')]
	private $never;

	/**
	 * @var \Doctrine\Common\Collections\Collection<AnotherEntity>
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'manyToOne')]
	private $genericCollection;

	/**
	 * @var \Doctrine\Common\Collections\Collection<mixed, AnotherEntity>
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'manyToOne')]
	private $genericCollection2;

	/**
	 * @var \Doctrine\Common\Collections\Collection<int, AnotherEntity>
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'manyToOne')]
	private $genericCollection3;

	/**
	 * @var \Doctrine\Common\Collections\Collection
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'manyToOne')]
	private $genericCollection4;

	/**
	 * @var \Doctrine\Common\Collections\Collection&iterable<MyEntity>
	 */
	#[ORM\OneToMany(targetEntity: 'PHPStan\Rules\Doctrine\ORM\AnotherEntity', mappedBy: 'manyToOne')]
	private $genericCollection5;

}
