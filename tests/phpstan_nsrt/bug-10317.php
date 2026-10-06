<?php declare(strict_types = 1);

namespace Bug10317;

use function PHPStan\Testing\assertType;

/**
 * @param array{'foo': string, 'bar'?: string} $array
 */
function myfunction(array $array): void
{
    $string = $array['bar'] ?? $array['foo'];
    assertType('string', $string);
}
