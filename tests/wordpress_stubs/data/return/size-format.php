<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;
use function size_format;

assertType('string', size_format(123));
assertType('string', size_format('123'));
assertType('string', size_format('123.1'));
assertType('string', size_format(0));
assertType('false', size_format('0'));
assertType('false', size_format('string'));
