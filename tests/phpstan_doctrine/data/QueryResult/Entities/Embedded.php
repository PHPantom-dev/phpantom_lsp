<?php declare(strict_types=1);

namespace QueryResult\Entities;

use Doctrine\Common\Collections\Collection;
use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Embeddable;
use Doctrine\ORM\Mapping\Embedded as ORMEmbedded;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;
use Doctrine\ORM\Mapping\JoinColumn;
use Doctrine\ORM\Mapping\ManyToOne;
use Doctrine\ORM\Mapping\OneToMany;

#[Embeddable]
class Embedded
{
	/**
	 * @var int
	 */
	#[Column(type: 'integer')]
	public $intColumn;

	/**
	 * @var string
	 */
	#[Column(type: 'string')]
	public $stringColumn;

	/**
	 * @var string|null
	 */
	#[Column(type: 'string', nullable: true)]
	public $stringNullColumn;

	/**
	 * @var NestedEmbedded
	 */
	#[ORMEmbedded(class: 'QueryResult\Entities\NestedEmbedded')]
	public $nestedEmbedded;
}
