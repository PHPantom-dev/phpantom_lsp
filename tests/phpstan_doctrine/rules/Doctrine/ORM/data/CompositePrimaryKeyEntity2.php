<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class CompositePrimaryKeyEntity2
{

	/**
	 * @var EntityWithRelations
	 */
	#[ORM\Id]
	#[ORM\ManyToOne(targetEntity: 'PHPStan\Rules\Doctrine\ORM\EntityWithRelations', inversedBy: 'countries')]
	private $post;

	/**
	 * @var string
	 */
	#[ORM\Id]
	#[ORM\Column(type: 'string')]
	private $country;

}
