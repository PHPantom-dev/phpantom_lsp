<?php

namespace TypesNamespaceTypehints;

use function PHPStan\Testing\assertType;

class FooWithAnonymousFunction
{

	public function doFoo()
	{
		function (
			Int $integer,
			boOl $boolean,
			String $string,
			Float $float,
			Lorem $loremObject,
			$mixed,
			Array $array,
			bool $isNullable = Null,
			Callable $callable,
			self $self
		) {
			assertType('int', $integer);
			assertType('bool', $boolean);
			assertType('string', $string);
			assertType('float', $float);
			assertType('TypesNamespaceTypehints\Lorem', $loremObject);
			assertType('mixed', $mixed);
			assertType('array', $array);
			assertType('bool|null', $isNullable);
			assertType('callable(): mixed', $callable);
			assertType('TypesNamespaceTypehints\FooWithAnonymousFunction', $self);
		};
	}

}
