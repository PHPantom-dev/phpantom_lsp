<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class GeneratedIdEntity5
{

	/**
	 * @var int
	 */
	#[ORM\Id]
	#[ORM\Column(type: 'integer', nullable: false)]
	private $id;

}
