<?php declare(strict_types = 1);

namespace Bug7153;

use Exception;
use function PHPStan\Testing\assertType;

function blah(): string
{
	return 'blah';
}

function bleh(): ?string
{
	return random_int(0, 1) > 0 ? 'bleh' : null;
}

function blih(string $blah, string $bleh): void
{
	echo 'test';
}

function () {
	$data = [blah(), bleh()];

	assertType('array{string, string|null}', $data);

	if (in_array(null, $data, true)) {
		assertType('array{string, string|null}', $data);
		throw new Exception();
	}

	assertType('array{string, string}', $data); // SKIP: a negated strict `in_array()` does not remove its needle from the haystack

	blih($data[0], $data[1]);
};
