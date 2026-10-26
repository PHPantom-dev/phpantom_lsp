<?php declare(strict_types = 1);

use function PHPStan\Testing\assertType;

$bag = new \Symfony\Component\HttpFoundation\InputBag(['foo' => 'bar', 'bar' => ['x']]);

assertType('bool|float|int|string|null', $bag->get('foo'));

if ($bag->has('foo')) {
	// Because `has` rely on `array_key_exists` we can still have set the NULL value.
	assertType('bool|float|int|string|null', $bag->get('foo'));
	assertType('bool|float|int|string|null', $bag->get('bar'));
} else {
	assertType('null', $bag->get('foo')); // SKIP: Symfony return types an argument decides
	assertType('bool|float|int|string|null', $bag->get('bar'));
}

assertType('bool|float|int|string|null', $bag->get('foo', null));
assertType('bool|float|int|string', $bag->get('foo', '')); // SKIP: Symfony return types an argument decides
assertType('bool|float|int|string', $bag->get('foo', 'baz')); // SKIP: Symfony return types an argument decides
assertType('array<string, array<mixed>|bool|float|int|string>', $bag->all()); // SKIP: Symfony return types an argument decides
assertType('array<mixed>', $bag->all('bar'));
