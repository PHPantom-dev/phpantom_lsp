<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;
use stdClass;

#[ORM\Entity]
class MyEntity
{
	/**
	 * @var int
	 */
	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column(type: 'integer')]
	private $id;

	#[ORM\Column(type: 'integer')]
	#[ORM\Version]
	private $version;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'string')]
	private $title;

	/**
	 * @var string
	 */
	private $transient;

	/**
	 * @var self
	 */
	#[ORM\ManyToOne(targetEntity: MyEntity::class)]
	private $parent;

	/**
	 * @var array
	 */
	#[ORM\Column(type: 'json')]
	private $jsonArray;

	/**
	 * @var bool|null
	 */
	#[ORM\Column(type: 'json')]
	private $jsonBoolOrNull;

	/**
	 * @var float
	 */
	#[ORM\Column(type: 'json')]
	private $jsonFloat;

	/**
	 * @var int
	 */
	#[ORM\Column(type: 'json')]
	private $jsonInt;

	/**
	 * @var JsonSerializableObject
	 */
	#[ORM\Column(type: 'json')]
	private $jsonSerializable;

	/**
	 * @var stdClass
	 */
	#[ORM\Column(type: 'json')]
	private $jsonStdClass;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'json')]
	private $jsonString;
}
