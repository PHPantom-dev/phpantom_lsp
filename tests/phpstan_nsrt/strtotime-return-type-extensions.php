<?php

namespace ClosureReturnTypeExtensionsNamespace;

use function PHPStan\Testing\assertType;

$strtotimeNow = strtotime('now');
assertType('int<1, max>', $strtotimeNow);

$strtotimeInvalid = strtotime('4 qm');
assertType('false', $strtotimeInvalid); // SKIP: `strtotime()` of a string it rejects is not folded to `false`

$strtotimeUnknown = strtotime(rand(0, 1) === 0 ? 'now': '4 qm');
assertType('int|false', $strtotimeUnknown);

$strtotimeUnknown2 = strtotime($undefined);
assertType('(int|false)', $strtotimeUnknown2);

$strtotimeCrash = strtotime();
assertType('int|false', $strtotimeCrash);

$strtotimeWithBase = strtotime('+2 days', time());
assertType('int', $strtotimeWithBase);

$strtotimePositiveInt = strtotime('1990-01-01 12:00:00 UTC');
assertType('int<1, max>', $strtotimePositiveInt);

$strtotimeNegativeInt = strtotime('1969-12-31 12:00:00 UTC');
assertType('int', $strtotimeNegativeInt);
