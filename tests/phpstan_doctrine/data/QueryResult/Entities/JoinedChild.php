<?php declare(strict_types=1);

namespace QueryResult\Entities;

use Doctrine\Common\Collections\Collection;
use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;
use Doctrine\ORM\Mapping\JoinColumn;
use Doctrine\ORM\Mapping\ManyToOne;
use Doctrine\ORM\Mapping\OneToMany;

#[Entity]
class JoinedChild extends JoinedParent
{
	/**
	 * @var int
	 */
	#[Column(type: 'integer')]
	public $childColumn;

	/**
	 * @var int
	 */
	#[Column(type: 'integer', nullable: true)]
	public $childNullColumn;
}
