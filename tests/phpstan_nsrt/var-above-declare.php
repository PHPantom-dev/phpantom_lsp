<?php

/**
 * @var string $foo
 */

declare(strict_types = 1);

\PHPStan\Testing\assertVariableCertainty(\PHPStan\TrinaryLogic::createYes(), $foo);
\PHPStan\Testing\assertType('string', $foo);
