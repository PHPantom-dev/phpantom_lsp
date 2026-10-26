<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class EntityWithCustomType
{

	/**
	 * @var int
	 */
	#[ORM\Id]
	#[ORM\Column(type: 'integer')]
	private $id;

	/**
	 * @var int
	 */
	#[ORM\Column(type: 'custom')]
	private $foo;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'custom_numeric')]
	private $numeric;

	/**
	 * @var numeric-string
	 */
	#[ORM\Column(type: 'custom_numeric')]
	private $correctNumeric;
}
