<?php declare(strict_types = 1);

namespace Bug7008;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function find(string $searchedValue, array $filters, bool $isAdmin): void
	{
		if ($searchedValue === '' || (!$isAdmin && empty($filters))) {
			// skip
			return;
		}

		// $searchedValue !== '' && ($isAdmin || !empty($filters))

		if (!$isAdmin) {
			// apply filters, no filters for admins
			assertType('non-empty-array', $filters);
		}
	}
}
