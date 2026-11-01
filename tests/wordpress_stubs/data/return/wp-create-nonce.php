<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_create_nonce;
use function PHPStan\Testing\assertType;

assertType('lowercase-string&non-falsy-string', wp_create_nonce(''));
assertType('lowercase-string&non-falsy-string', wp_create_nonce('action'));
assertType('lowercase-string&non-falsy-string', wp_create_nonce(Faker::string()));
assertType('lowercase-string&non-falsy-string', wp_create_nonce(-1));
