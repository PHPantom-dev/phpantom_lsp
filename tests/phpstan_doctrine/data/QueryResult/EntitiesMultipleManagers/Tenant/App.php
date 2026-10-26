<?php declare(strict_types = 1);

namespace QueryResult\MultipleEntityManagers\Tenant;

use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;

#[Entity]
class App
{

	/**
	 * @var int
	 */
	#[Column(type: 'integer')]
	#[Id]
	public $id;

}
