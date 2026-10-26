<?php declare(strict_types=1);

namespace QueryResult\Entities;

use Doctrine\Common\Collections\Collection;
use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Embedded as ORMEmbedded;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;
use Doctrine\ORM\Mapping\JoinColumn;
use Doctrine\ORM\Mapping\ManyToOne;
use Doctrine\ORM\Mapping\OneToMany;
use QueryResult\Entities\One;

#[Entity]
class CompoundPkAssoc
{
	/**
	 * @var One
	 */
	#[ManyToOne(targetEntity: 'QueryResult\Entities\One')]
	#[JoinColumn(nullable: false)]
	#[Id]
	public $one;

	/**
	 * @var int
	 */
	#[Column(type: 'integer')]
	#[Id]
	public $version;
}
