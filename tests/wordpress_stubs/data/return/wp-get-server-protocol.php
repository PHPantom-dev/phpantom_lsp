<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_server_protocol;
use function PHPStan\Testing\assertType;

assertType("'HTTP/1.0'|'HTTP/1.1'|'HTTP/2'|'HTTP/2.0'|'HTTP/3'", wp_get_server_protocol());
assertType('false', wp_get_server_protocol() === 'HTTP/1');
assertType('bool', wp_get_server_protocol() === 'HTTP/3');
assertType('bool', wp_get_server_protocol() === Faker::string());
