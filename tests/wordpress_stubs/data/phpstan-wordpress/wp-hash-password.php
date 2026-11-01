<?php

declare(strict_types=1);

namespace SzepeViktor\PHPStan\WordPress\Tests;

use function PHPStan\Testing\assertType;
use function wp_hash_password;

/*
 * PHP 8.0 or later: password_hash() throws instead of returning null or false.
 */

assertType('non-falsy-string', wp_hash_password('password'));

/** @var string $password */
assertType('non-falsy-string', wp_hash_password($password));
