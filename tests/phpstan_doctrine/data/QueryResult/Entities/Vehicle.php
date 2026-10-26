<?php
declare(strict_types=1);

namespace Type\Doctrine\data\QueryResult\Entities;

use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\GeneratedValue;
use Doctrine\ORM\Mapping\Id;

interface VehicleInterface
{

}

#[Entity]
class Car implements VehicleInterface
{
	/**
	 * @var string
	 */
	#[GeneratedValue]
	#[Column(type: 'integer')]
	#[Id]
	public $id;
}

#[Entity]
class Truck implements VehicleInterface
{
	/**
	 * @var string
	 */
	#[GeneratedValue]
	#[Column(type: 'integer')]
	#[Id]
	public $id;
}
