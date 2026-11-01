<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function current_action;
use function PHPStan\Testing\assertType;

assertType('non-empty-string|false', current_action());
