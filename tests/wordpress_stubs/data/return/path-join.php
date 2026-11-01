<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function path_join;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', path_join('', ''));
assertType('non-falsy-string', path_join('base', 'path'));
assertType('non-falsy-string', path_join(Faker::string(), Faker::string()));
