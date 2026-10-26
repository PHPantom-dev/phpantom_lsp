<?php // lint >= 8.1

namespace PHPStan\Rules\Doctrine\ORM\Bug679;

use Doctrine\ORM\Mapping as ORM;

enum FooEnum: string {

	case ONE = 'one';
	case TWO = 'two';

}

#[ORM\Entity]
class MyBrokenEntity
{
	/**
	 * @var int|null
	 */
	#[ORM\Id]
	#[ORM\GeneratedValue]
	#[ORM\Column(type: 'integer')]
	private $id;

	#[ORM\Column(type: "enum", enumType: FooEnum::class)]
	public FooEnum $type1;
}
