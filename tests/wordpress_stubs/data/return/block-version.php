<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function block_version;
use function PHPStan\Testing\assertType;

// Returns input for non-negative integers
assertType('0', block_version(''));
assertType('0|1', block_version('content'));
assertType('0|1', block_version(Faker::string()));
