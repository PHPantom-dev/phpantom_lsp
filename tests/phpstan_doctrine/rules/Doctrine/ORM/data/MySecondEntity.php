<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity(repositoryClass: 'PHPStan\Rules\Doctrine\ORM\TestRepository')]
class MySecondEntity
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
	#[ORM\Column(type: 'string')]
	private $title;

	/**
	 * @var string
	 */
	private $transient;

}
