<?php declare(strict_types=1);

namespace QueryResult\EntitiesDbal42;

use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\GeneratedValue;
use Doctrine\ORM\Mapping\Id;

#[Entity]
class Dbal4Entity
{

	/**
	 * @var int
	 */
	#[Column(type: 'integer')]
	#[GeneratedValue(strategy: 'AUTO')]
	#[Id]
	public $id;

	/**
	 * @var string
	 */
	#[Column(type: 'enum', options: ['values' => ['a', 'b', 'c']])]
	public $enum;  // dbal 4.2+

	/**
	 * @var float
	 */
	#[Column(type: 'smallfloat')]
	public $smallfloat; // dbal 4.1+
}
