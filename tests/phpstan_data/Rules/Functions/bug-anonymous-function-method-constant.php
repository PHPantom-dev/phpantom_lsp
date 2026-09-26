<?php declare(strict_types = 1);

namespace BugAnonymousFunctionMethodConstant;

$a = fn() => __FUNCTION__;
$b = fn() => __METHOD__;

$c = function() { return __FUNCTION__; };
$d = function() { return __METHOD__; };

\PHPStan\Testing\assertType("'{closure}'", $a()); // SKIP: a magic constant resolves to its base type rather than its value
\PHPStan\Testing\assertType("'{closure}'", $b()); // SKIP: a magic constant resolves to its base type rather than its value
\PHPStan\Testing\assertType("'{closure}'", $c()); // SKIP: a magic constant resolves to its base type, and calling a closure without a declared return type gives mixed
\PHPStan\Testing\assertType("'{closure}'", $d()); // SKIP: a magic constant resolves to its base type, and calling a closure without a declared return type gives mixed
