<?php

/**
 * @var string $foo
 */

use Foo\Bar\Baz;

\PHPStan\Testing\assertVariableCertainty(\PHPStan\TrinaryLogic::createYes(), $foo);
\PHPStan\Testing\assertType('string', $foo);
