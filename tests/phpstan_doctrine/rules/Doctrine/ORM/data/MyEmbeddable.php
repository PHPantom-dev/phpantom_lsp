<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Embeddable]
class MyEmbeddable
{
	/**
	 * @var string
	 */
	#[ORM\Column(type: 'string')]
	private $title;
}
