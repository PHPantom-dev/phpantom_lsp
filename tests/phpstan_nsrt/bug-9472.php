<?php // lint >= 8.0

namespace Bug9472;

use Closure;
use function PHPStan\Testing\assertType;

/**
 * @template Tk
 * @template Tv
 * @template T
 * @param array<Tk, Tv> $iterable
 * @param (Closure(Tv): T) $function
 *
 * @return list<T>
 */
function map(array $iterable, Closure $function): array
{
	$result = [];
	foreach ($iterable as $value) {
		$result[] = $function($value);
	}

	return $result;
}

function (): void {
	/** @var list<non-empty-string> */
	$nonEmptyStrings = [];

	map($nonEmptyStrings, static function (string $variable) {
		return $variable;
	});
};

/**
 * @template Type
 * @param Type $x
 * @return Type
 */
function identity($x) {
	return $x;
}

function (): void {
	$x = rand() > 5 ? 'a' : 'b';
	assertType('\'a\'|\'b\'', $x);
	$y = identity($x);
	assertType('\'a\'|\'b\'', $y);
};

/**
 * @template ParseResultType
 * @param callable():ParseResultType $parseFunction
 * @return ParseResultType|null
 */
function tryParse(callable $parseFunction) {
	try {
		return $parseFunction();
	} catch (\Exception $e) {
		return null;
	}
}

/** @return array{type: 'typeA'|'typeB'} */
function parseData(mixed $data): array {
	return ['type' => 'typeA'];
}

function (): void {
	$data = tryParse(fn() => parseData('whatever'));
	assertType('array{type: \'typeA\'|\'typeB\'}|null', $data);
};
