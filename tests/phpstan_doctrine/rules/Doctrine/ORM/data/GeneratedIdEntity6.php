<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class GeneratedIdEntity6
{

	/**
	 * @var int|null
	 */
	#[ORM\Id]
	#[ORM\Column(type: 'integer', nullable: false)]
	private $id;

}
