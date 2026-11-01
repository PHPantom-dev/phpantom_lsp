<?php

/**
 * `rememberPossiblyImpureFunctionValues` set to true (default).
 */

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests\ImpureDemo;

use function PHPStan\Testing\assertType;

function pureByDefault(): bool {
    return true;
}

/** @phpstan-impure */
function impureByTag(): bool {
    return (bool)random_int(0, 1);
}

if (pureByDefault() === true) {
    assertType('true', pureByDefault()); // True
}

if (impureByTag() === true) {
    assertType('bool', impureByTag()); // Bool
}
