<?php declare(strict_types = 1);

namespace Bug14275;

use function PHPStan\Testing\assertType;

// Basic reference: modifying $b should update $a
$a = [];
$b = &$a;

$b[0] = 1;
assertType('array{1}', $a);
assertType('array{1}', $b);

// Reference with scalar reassignment
$c = 1;
$d = &$c;
$d = 2;
assertType('2', $c);
assertType('2', $d);

// Reference with different type reassignment
$e = 'hello';
$f = &$e;
$f = 42;
assertType('42', $e);
assertType('42', $f);

// Subsequent assignments should continue propagating
$e = 22;
assertType('22', $e);
assertType('22', $f);

$f = 33;
assertType('33', $e);
assertType('33', $f);
