<?php declare(strict_types=1);

namespace QueryResult\Entities;

use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;
use Doctrine\ORM\Mapping\JoinColumn;
use Doctrine\ORM\Mapping\ManyToOne;
use Doctrine\ORM\Mapping as ORM;

#[Entity]
class Many
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
	 * @var \DateTime
	 */
	#[Column(type: 'datetime')]
	public $datetimeColumn;

	/**
	 * @var \DateTimeImmutable
	 */
	#[Column(type: 'datetime_immutable')]
	public $datetimeImmutableColumn;

	/**
	 * @var One
	 */
	#[ManyToOne(targetEntity: 'QueryResult\Entities\One', inversedBy: 'manies')]
	#[JoinColumn(nullable: false)]
	public $one;

	/**
	 * @var One|null
	 */
	#[ManyToOne(targetEntity: 'QueryResult\Entities\One')]
	#[JoinColumn(nullable: true)]
	public $oneNull;

	/**
	 * @var One|null
	 */
	#[ManyToOne(targetEntity: 'QueryResult\Entities\One')]
	public $oneDefaultNullability;

	/**
	 * @var CompoundPk|null
	 */
	#[ManyToOne(targetEntity: 'QueryResult\Entities\CompoundPk')]
	#[JoinColumn(name: 'compoundPk_id', referencedColumnName: 'id')]
	#[JoinColumn(name: 'compoundPk_version', referencedColumnName: 'version')]
	public $compoundPk;

	/**
	 * @var CompoundPkAssoc|null
	 */
	#[ManyToOne(targetEntity: 'QueryResult\Entities\CompoundPkAssoc')]
	#[JoinColumn(name: 'compoundPk_one', referencedColumnName: 'one_id')]
	#[JoinColumn(name: 'compoundPk_version', referencedColumnName: 'version')]
	public $compoundPkAssoc;

	/**
	 * @var list<string>
	 */
	#[ORM\Column(type: 'simple_array')]
	public $simpleArrayColumn;
}

#[ORM\Entity]
class Bug245Episode
{
	/**
	 * @var string
	 */
	#[Column(type: 'bigint')]
	#[Id]
	public $id;

	/**
	 * @var \Doctrine\Common\Collections\Collection<Bug245Segment>
	 */
	#[ORM\OneToMany(targetEntity: '\QueryResult\Entities\Bug245Segment', mappedBy: 'episode', cascade: ['persist', 'remove'], orphanRemoval: true)]
	#[ORM\OrderBy(['position' => 'ASC'])]
	private $segments;
}

#[ORM\Entity]
class Bug245Segment
{
	/**
	 * @var string
	 */
	#[Column(type: 'bigint')]
	#[Id]
	public $id;

	/**
	 * @var Bug245Episode
	 */
	#[ORM\ManyToOne(targetEntity: '\QueryResult\Entities\Bug245Episode', inversedBy: 'segments', cascade: ['persist'])]
	#[ORM\JoinColumn(name: 'episode_id', referencedColumnName: 'id', nullable: false)]
	private $episode;
}
