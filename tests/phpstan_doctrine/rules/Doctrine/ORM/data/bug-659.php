<?php // lint >= 8.0

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class MyEntity659
{

	/**
	 * @var int
	 */
	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column(type: 'integer')]
	private $id;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'binary')]
	private $binaryString;

	/**
	 * @var resource
	 */
	#[ORM\Column(type: 'binary')]
	private $binaryResource;
}
