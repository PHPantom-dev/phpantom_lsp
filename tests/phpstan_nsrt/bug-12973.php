<?php

namespace Bug12973;

use function PHPStan\Testing\assertType;

/**
 * @return non-empty-string|null
 */
function nullIfBlank(?string $value): ?string
{
	if ($value !== null) {
		return \trim($value) === '' ? null : $value;
	}

	return null;
}

function trimMixed($value): void
{
	if (trim($value) === '') {
		assertType('mixed', $value);
	} else {
		assertType('mixed', $value);
	}
	assertType('mixed', $value);

}

function trimTypes(string $value): void
{
	if (trim($value) === '') {
		assertType('string', $value);
	} else {
	}
	assertType('string', $value);

	if (trim($value) !== '') {
	} else {
		assertType('string', $value);
	}
	assertType('string', $value);
}

function ltrimTypes(string $value): void
{
	if (ltrim($value) === '') {
		assertType('string', $value);
	} else {
	}
	assertType('string', $value);

	if (ltrim($value) !== '') {
	} else {
		assertType('string', $value);
	}
	assertType('string', $value);
}

function rtrimTypes(string $value): void
{
	if (rtrim($value) === '') {
		assertType('string', $value);
	} else {
	}
	assertType('string', $value);

	if (rtrim($value) !== '') {
	} else {
		assertType('string', $value);
	}
	assertType('string', $value);
}

function chopTypes(string $value): void
{
	if (chop($value) === '') {
		assertType('string', $value);
	} else {
	}
	assertType('string', $value);

	if (chop($value) !== '') {
	} else {
		assertType('string', $value);
	}
	assertType('string', $value);
}
