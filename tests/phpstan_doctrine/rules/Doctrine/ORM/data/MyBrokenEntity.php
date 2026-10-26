<?php declare(strict_types = 1);

namespace PHPStan\Rules\Doctrine\ORM;

use Doctrine\ORM\Mapping as ORM;

#[ORM\Entity]
class MyBrokenEntity extends MyBrokenSuperclass
{

	/**
	 * @var int|null
	 */
	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column(type: 'bigint')]
	private $id;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'string', nullable: true)]
	private $one;

	/**
	 * @var string|null
	 */
	#[ORM\Column(type: 'string')]
	private $two;

	/**
	 * @var \DateTimeImmutable
	 */
	#[ORM\Column(type: 'datetime')]
	private $three;

	/**
	 * @var \DateTime
	 */
	#[ORM\Column(type: 'datetime_immutable')]
	private $four;

	/**
	 * @var \DateTimeImmutable
	 */
	#[ORM\Column(type: 'date')]
	private $six;

	#[ORM\Column(type: 'date')]
	private $mixed;

	/**
	 * @var int&string
	 */
	#[ORM\Column(type: 'date')]
	private $never;

	/**
	 * @var \Ramsey\Uuid\UuidInterface
	 */
	#[ORM\Column(type: 'uuid')]
	private $uuid;

	/**
	 * @var int
	 */
	#[ORM\Column(type: 'uuid')]
	private $uuidInvalidType;

	/**
	 * @var int[]
	 */
	#[ORM\Column(type: 'array')]
	private $arrayOfIntegers;

	/**
	 * @var mixed[][]
	 */
	#[ORM\Column(type: 'array')]
	private $arrayOfArrays;

	/**
	 * @var mixed[]
	 */
	#[ORM\Column(type: 'array')]
	private $mixeds;

	/**
	 * @var array|null
	 */
	#[ORM\Column(type: 'array')]
	private $arrayOrNull;

	/**
	 * @var int[]|null
	 */
	#[ORM\Column(type: 'array')]
	private $arrayOfIntegersOrNull;

	/**
	 * @var int|float|numeric-string
	 */
	#[ORM\Column(type: 'decimal')]
	private $decimal;

	/**
	 * @var int|float|string
	 */
	#[ORM\Column(type: 'decimal')]
	private $decimalWithString;

	/**
	 * @var string
	 */
	#[ORM\Column(type: 'decimal')]
	private $decimalWithString2;

	/**
	 * @var numeric-string
	 */
	#[ORM\Column(type: 'string')]
	private $numericString;

	/**
	 * @var \Carbon\CarbonImmutable
	 */
	#[ORM\Column(type: 'carbon')]
	private $invalidCarbon;

	/**
	 * @var \Carbon\Carbon
	 */
	#[ORM\Column(type: 'carbon_immutable')]
	private $invalidCarbonImmutable;

	/**
	 * @var \Carbon\Carbon
	 */
	#[ORM\Column(type: 'carbon')]
	private $validCarbon;

	/**
	 * @var \Carbon\CarbonImmutable
	 */
	#[ORM\Column(type: 'carbon_immutable')]
	private $validCarbonImmutable;

	/**
	 * @var EmptyObject
	 */
	#[ORM\Column(type: 'json')]
	private $incompatibleJsonValueObject;

	/**
	 * @var int[]
	 */
	#[ORM\Column(type: 'simple_array')]
	private $invalidSimpleArray;

	/**
	 * @var list<string>
	 */
	#[ORM\Column(type: 'simple_array')]
	private $validSimpleArray;
}
