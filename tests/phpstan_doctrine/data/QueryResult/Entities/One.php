<?php declare(strict_types=1);

namespace QueryResult\Entities;

use Doctrine\Common\Collections\Collection;
use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Embedded as ORMEmbedded;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;
use Doctrine\ORM\Mapping\JoinColumn;
use Doctrine\ORM\Mapping\OneToMany;
use Doctrine\ORM\Mapping\OneToOne;

#[Entity]
class One
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
	public $intColumn;

	/**
	 * @var float
	 */
	#[Column(type: 'float')]
	public $floatColumn;

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
	 * @var SubOne
	 */
	#[OneToOne(targetEntity: 'QueryResult\Entities\SubOne', cascade: ['persist'])]
	#[JoinColumn(nullable: false)]
	public $subOne;

	/**
	 * @var Collection<int,Many>
	 */
	#[OneToMany(targetEntity: 'QueryResult\Entities\Many', mappedBy: 'one')]
	public $manies;

	/**
	 * @var Embedded
	 */
	#[ORMEmbedded(class: 'QueryResult\Entities\Embedded')]
	public $embedded;

	/**
	 * @var mixed
	 */
	#[Column(type: 'custom_int', nullable: true)]
	public $mixedColumn;

	public function __construct()
	{
		$this->subOne = new SubOne();
	}
}
