<?php declare(strict_types = 1);

namespace BugAnonymousFunctionMethodConstant;

$a = fn() => __FUNCTION__;
$b = fn() => __METHOD__;

$c = function() { return __FUNCTION__; };
$d = function() { return __METHOD__; };

\PHPStan\Testing\assertType("'{closure}'", $a());
\PHPStan\Testing\assertType("'{closure}'", $b());
\PHPStan\Testing\assertType("'{closure}'", $c());
\PHPStan\Testing\assertType("'{closure}'", $d());
