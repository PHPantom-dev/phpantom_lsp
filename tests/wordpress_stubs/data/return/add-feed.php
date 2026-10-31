<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function add_feed;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', add_feed('', Faker::callable()));
assertType('non-falsy-string', add_feed('value', Faker::callable()));
assertType('non-falsy-string', add_feed(Faker::nonEmptyString(), Faker::callable()));
assertType('non-falsy-string', add_feed(Faker::nonFalsyString(), Faker::callable()));
