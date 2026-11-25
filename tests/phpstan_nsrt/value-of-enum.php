<?php // lint >= 8.1

declare(strict_types=1);

namespace ValueOfEnum;

use function PHPStan\Testing\assertType;

enum Country: string
{
	case NL = 'The Netherlands';
	case US = 'United States';
}

class Foo {
	/**
	 * @return value-of<Country>
	 */
	function us()
	{
		return Country::US;
	}

	/**
	 * @param value-of<Country> $countryName
	 */
	function hello($countryName)
	{
		assertType("'The Netherlands'|'United States'", $countryName);
	}

	function doFoo() {
		assertType("'The Netherlands'|'United States'", $this->us());
	}
}

