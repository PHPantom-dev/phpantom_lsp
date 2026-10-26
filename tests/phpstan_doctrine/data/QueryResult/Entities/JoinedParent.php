<?php declare(strict_types=1);

namespace QueryResult\Entities;

use Doctrine\Common\Collections\Collection;
use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\DiscriminatorColumn;
use Doctrine\ORM\Mapping\DiscriminatorMap;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;
use Doctrine\ORM\Mapping\InheritanceType;
use Doctrine\ORM\Mapping\JoinColumn;
use Doctrine\ORM\Mapping\ManyToOne;
use Doctrine\ORM\Mapping\OneToMany;

#[Entity]
#[InheritanceType('JOINED')]
#[DiscriminatorColumn(name: 'discr', type: 'string')]
#[DiscriminatorMap(['child' => 'QueryResult\Entities\JoinedChild'])]
abstract class JoinedParent
{
	/**
	 * @var string
	 */
	#[Column(type: 'bigint')]
	#[Id]
	public $id;

	/**
	 * @var int
	 */
	#[Column(type: 'integer')]
	public $parentColumn;

	/**
	 * @var int
	 */
	#[Column(type: 'integer', nullable: true)]
	public $parentNullColumn;
}
