<?php // lint >= 8.1

namespace Bug9007;

use function PHPStan\Testing\assertType;

enum Country: string {
	case Usa = 'USA';
	case Canada = 'CAN';
	case Mexico = 'MEX';
}

function doStuff(string $countryString): int {
	assertType(Country::class, Country::from($countryString));
	return match (Country::from($countryString)) {
		Country::Usa => 1,
		Country::Canada => 2,
		Country::Mexico => 3,
	};
}
