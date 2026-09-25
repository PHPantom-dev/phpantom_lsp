<?php

use function PHPStan\Testing\assertType;

/** @var bool|null $boolOrNull */
$boolOrNull = doFoo();
$bool = $boolOrNull !== null ? $boolOrNull : false;

$short = $bool ?: null;

/** @var bool $a */
$a = doBar();
/** @var bool $b */
$b = doBaz();
$c = $a ?: $b;

/** @var string|null $qux */
$qux = doQux();
$isQux = $qux !== null ?: $bool;

assertType('bool|null', $boolOrNull);
assertType('bool', $boolOrNull !== null ? $boolOrNull : false);
assertType('bool', $bool);
assertType('true|null', $short);
assertType('bool', $c);
assertType('bool', $isQux);
