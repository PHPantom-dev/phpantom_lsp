<?php // lint >= 8.1

namespace PHPStan\Rules\Doctrine\ORMAttributes;

use Doctrine\ORM\Mapping as ORM;

enum FooEnum: string {

	case ONE = 'one';
	case TWO = 'two';

}

enum BarEnum: string {

	case ONE = 'one';
	case TWO = 'two';

}

enum BazEnum: int {

	case ONE = 1;
	case TWO = 2;

}

#[ORM\Entity]
class Foo
{


	#[ORM\Column(nullable: false)]
	#[ORM\Id]
	#[ORM\GeneratedValue]
	private ?int $id = null;

	#[ORM\Column(type: "string", enumType: FooEnum::class)]
	public FooEnum $type1;

	#[ORM\Column(type: "string", enumType: FooEnum::class)]
	public BarEnum $type2;

	#[ORM\Column(type: "integer", enumType: FooEnum::class)]
	public FooEnum $type3;

	#[ORM\Column]
	public FooEnum $type4;

	#[ORM\Column(type: "simple_array", enumType: FooEnum::class)]
	public FooEnum $type5;

	/**
	 * @var list<FooEnum>
	 */
	#[ORM\Column(type: "simple_array", enumType: FooEnum::class)]
	public array $type6;

	/**
	 * @var list<BazEnum>
	 */
	#[ORM\Column(type: "simple_array", enumType: BazEnum::class)]
	public array $type7;
}
