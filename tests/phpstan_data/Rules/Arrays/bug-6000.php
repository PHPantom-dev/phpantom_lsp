<?php declare(strict_types = 1);

namespace Bug6000;

use function PHPStan\Testing\assertType;

function (): void {
	/** @var array{psr-4?: array<string, string|string[]>, classmap?: list<string>} $data */
	$data = [];

	foreach ($data as $key => $value) {
		// PHPantom is more precise than PHPStan here: it keeps the two shape values apart rather than joining them into one array.
		assertType('array<string, string|array<string>>|list<string>', $data[$key]);
		if ($key === 'classmap') {
			assertType('list<string>', $data[$key]); // SKIP: comparing a foreach key does not narrow the value it was read with
			assertType('list<string>', $value); // SKIP: comparing a foreach key does not narrow the value it was read with
			echo implode(', ', $value); // not working :(
			echo implode(', ', $data[$key]); // this works though?!
		}
	}
};
