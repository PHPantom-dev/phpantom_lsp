<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function did_action;
use function PHPStan\Testing\assertType;

assertType('int<0, max>', did_action('hook_name'));
