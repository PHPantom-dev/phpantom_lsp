<?php

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;
use Symfony\Component\Uid\Ulid;
use Symfony\Component\Uid\Uuid;

#[ORM\Entity]
class EntityWithSymfonyUid
{
	/**
	 * @var int
	 */
	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column(type: 'integer')]
	private $id;

	/**
	 * @var Uuid
	 */
	#[ORM\Column(type: 'uuid')]
	private $uuid;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'uuid')]
	private $uuidInvalidType;

	/**
	 * @var Ulid
	 */
	#[ORM\Column(type: 'ulid')]
	private $ulid;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'ulid')]
	private $ulidInvalidType;
}
