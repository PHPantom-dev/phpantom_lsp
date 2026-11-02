<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function did_filter;
use function PHPStan\Testing\assertType;

assertType('int<0, max>', did_filter('hook_name'));
