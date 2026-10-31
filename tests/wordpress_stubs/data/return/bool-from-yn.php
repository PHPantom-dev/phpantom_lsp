<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function bool_from_yn;
use function PHPStan\Testing\assertType;

assertType('true', bool_from_yn('y'));
assertType('false', bool_from_yn('n'));
assertType('false', bool_from_yn('thing'));
assertType('bool', bool_from_yn(Faker::string()));
