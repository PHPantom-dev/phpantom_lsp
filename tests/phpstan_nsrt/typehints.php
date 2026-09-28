<?php // lint >= 8.0

namespace TypesNamespaceTypehints;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo(
		int $integer,
		bool $boolean,
		string $string,
		float $float,
		Lorem $loremObject,
		$mixed,
		array $array,
		bool $isNullable = null,
		callable $callable,
		string ...$variadicStrings
	): Bar
	{
		$loremObjectRef = $loremObject;
		$barObject = $this->doFoo();
		$fooObject = new self();
		$anotherBarObject = $fooObject->doFoo();
		assertType('int', $integer);
		assertType('bool', $boolean);
		assertType('string', $string);
		assertType('float', $float);
		assertType('TypesNamespaceTypehints\Lorem', $loremObject);
		assertType('mixed', $mixed);
		assertType('array', $array);
		assertType('bool|null', $isNullable);
		assertType('TypesNamespaceTypehints\Lorem', $loremObjectRef);
		assertType('TypesNamespaceTypehints\Bar', $barObject);
		assertType('TypesNamespaceTypehints\Foo', $fooObject);
		assertType('TypesNamespaceTypehints\Bar', $anotherBarObject);
		assertType('callable(): mixed', $callable);
		assertType('array<int|string, string>', $variadicStrings);
		assertType('string', $variadicStrings[0]);
	}

}
