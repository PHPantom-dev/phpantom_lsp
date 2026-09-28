<?php

namespace SlevomatForeachUnsetBug;

use function PHPStan\Testing\assertType;

class Foo
{

	/** @var array{items: array<string, \stdClass>, isActive: bool, productsCount: int} */
	private $foreignSection;

	public function doFoo()
	{
		// Detect if foreign countries are visible
		foreach ($this->foreignSection['items'] as $foreignCountryNo => $foreignCountryItem) {
			if ($foreignCountryItem->count > 0) {
				continue;
			}

			assertType('array{items: array<string, stdClass>, isActive: bool, productsCount: int}', $this->foreignSection);
			assertType('array<string, stdClass>', $this->foreignSection['items']);
			unset($this->foreignSection['items'][$foreignCountryNo]);
			assertType('array{items: array<string, stdClass>, isActive: bool, productsCount: int}', $this->foreignSection);
			assertType('array<string, stdClass>', $this->foreignSection['items']);
		}

		assertType('array{items: array<string, stdClass>, isActive: bool, productsCount: int}', $this->foreignSection);
		assertType('array<string, stdClass>', $this->foreignSection['items']);
		$countriesItems = $this->foreignSection['items'];
	}

}
