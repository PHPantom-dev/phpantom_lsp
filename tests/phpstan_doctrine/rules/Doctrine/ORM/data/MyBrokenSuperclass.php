<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\MappedSuperclass]
abstract class MyBrokenSuperclass
{

	/**
	 * @var int
	 */
	#[ORM\Column(type: 'binary')]
	private $five;

}
