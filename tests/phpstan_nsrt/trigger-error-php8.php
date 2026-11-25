<?php // lint >= 8.0

declare(strict_types = 1);

namespace TriggerErrorPhp8;

use function PHPStan\Testing\assertType;

$errorLevels = [E_USER_DEPRECATED, E_USER_ERROR, E_USER_NOTICE, E_USER_WARNING, E_NOTICE, E_WARNING];

assertType('true', trigger_error('bar'));
assertType('true', trigger_error('bar', $errorLevels[0]));
assertType('*NEVER*', trigger_error('bar', $errorLevels[1]));
assertType('true', trigger_error('bar', $errorLevels[2]));
assertType('true', trigger_error('bar', $errorLevels[3]));
