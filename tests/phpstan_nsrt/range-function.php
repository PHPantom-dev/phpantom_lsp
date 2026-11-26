<?php // lint >= 8.3

use function PHPStan\Testing\assertType;

/** @var int $integer */
$integer = doFoo();

/** @var float $float */
$float = doFoo();

/** @var mixed $mixed */
$mixed = doFoo();

assertType('array{2, 3, 4, 5}', range(2, 5));
assertType('array{2, 4}', range(2, 5, 2));
assertType('array{2, 0}', range(2, '', 2));
assertType('array{2, 3, 4, 5}', range(2, 5, 1.0));
assertType('array{2.1, 3.1, 4.1}', range(2.1, 5));
// More precise than upstream, which reports `non-empty-list<int>`: every
// value lies between the bounds.
assertType('non-empty-list<int<2, 5>>', range(2, 5, $integer));
// More precise than upstream, which reports `non-empty-list<float|int>`: a
// float bound makes every value a float.
assertType('non-empty-list<float>', range($float, 5, $integer));
assertType('non-empty-list<(float|int|string)>', range($float, $mixed, $integer));
assertType('non-empty-list<(float|int|string)>', range($integer, $mixed));
assertType('array{0: 1, 1?: 2}', range(1, doFoo() ? 1 : 2));
assertType('array{0: -1, 1: 0, 2: 1, 3?: 2}|array{0: 1, 1?: 2}', range(doFoo() ? -1 : 1, doFoo() ? 1 : 2));
assertType('array{3, 2, 1, 0, -1}', range(3, -1));
assertType('non-empty-list<int<0, 50>>', range(0, 50));
