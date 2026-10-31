<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function rest_authorization_required_code;
use function PHPStan\Testing\assertType;

assertType('401|403', rest_authorization_required_code());
