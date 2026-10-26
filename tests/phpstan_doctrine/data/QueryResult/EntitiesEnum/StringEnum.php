<?php declare(strict_types=1); // lint >= 8.1

namespace QueryResult\EntitiesEnum;

use Doctrine\Common\Collections\Collection;
use Doctrine\ORM\Mapping\Column;
use Doctrine\ORM\Mapping\Embedded as ORMEmbedded;
use Doctrine\ORM\Mapping\Entity;
use Doctrine\ORM\Mapping\Id;
use Doctrine\ORM\Mapping\JoinColumn;
use Doctrine\ORM\Mapping\ManyToOne;
use Doctrine\ORM\Mapping\OneToMany;

enum StringEnum: string
{
	case A = 'a';
	case B = 'b';
}
