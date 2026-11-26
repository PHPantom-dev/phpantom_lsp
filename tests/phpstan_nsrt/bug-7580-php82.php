<?php // lint >= 8.2

declare(strict_types = 1);

namespace Bug7580TypesPHP82;

use function PHPStan\Testing\assertType;

assertType('array{}', mb_str_split('', 1));

assertType('array{\'x\'}', mb_str_split('x', 1));

$v = (string) (mt_rand() === 0 ? '' : 'x');
assertType('\'\'|\'x\'', $v);
assertType('array{}|array{\'x\'}', mb_str_split($v, 1));

function x(): string { throw new \Exception(); };
$v = x();
assertType('string', $v);
assertType('list<non-empty-string>', mb_str_split($v, 1));
