<?php

/**
 * `rememberPossiblyImpureFunctionValues` set to false.
 */

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests\PureDemo;

use function PHPStan\Testing\assertType;

/** @phpstan-pure */
function pureByTag(): bool {
    return true;
}

function impureByConfig(): bool {
    return (bool)random_int(0, 1);
}

if (pureByTag() === true) {
    assertType('true', pureByTag());
}

if (impureByConfig() === true) {
    assertType('true', impureByConfig()); // PHPantom remembers a call's result unless the function is marked impure, PHPStan's default; upstream runs pure/ with rememberPossiblyImpureFunctionValues off
}
