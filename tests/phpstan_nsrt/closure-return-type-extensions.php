<?php

namespace ClosureReturnTypeExtensionsNamespace;

use function PHPStan\Testing\assertType;

$predicate = function (object $thing): bool { return true; };

$closure = \Closure::fromCallable($predicate);
assertType('Closure(object): true', $closure);

$newThis = new class {};
$boundClosure = $closure->bindTo($newThis);
assertType('Closure(object): true', $boundClosure);

$staticallyBoundClosure = \Closure::bind($closure, $newThis);
assertType('Closure(object): true', $staticallyBoundClosure);

$returnType = $closure->call($newThis, new class {});
assertType('true', $returnType);

$staticallyBoundClosureCaseInsensitive = \closure::bind($closure, $newThis);
assertType('Closure(object): true', $staticallyBoundClosureCaseInsensitive);
