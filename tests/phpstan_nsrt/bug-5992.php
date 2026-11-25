<?php declare(strict_types=1);

namespace Bug5992;

use function PHPStan\Testing\assertType;

function StringValue(): string
{
	$values = [1, 'one', true, null, []];
	$value = $values[rand(0, 4)];
	if (!is_string($value)) {
		assertType('true', trigger_error("just a soft warning", E_USER_WARNING));
		assertType('*NEVER*', trigger_error("error which halts the script", E_USER_ERROR));
	}
	return $value;
}

function foo(?string $s): void
{
	if ($s === null) {
		trigger_error('foo', E_USER_ERROR);
	}
	assertType('string', $s);
}
