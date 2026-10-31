<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_hash;
use function wp_fast_hash;
use function PHPStan\Testing\assertType;

assertType('lowercase-string&non-falsy-string', wp_hash(''));
assertType('lowercase-string&non-falsy-string', wp_hash('data'));
assertType('lowercase-string&non-falsy-string', wp_hash(Faker::string()));
assertType('lowercase-string&non-falsy-string', wp_hash('', 'scheme'));
assertType('lowercase-string&non-falsy-string', wp_hash('data', 'scheme'));
assertType('lowercase-string&non-falsy-string', wp_hash(Faker::string(), 'scheme'));
assertType('lowercase-string&non-falsy-string', wp_hash('', 'scheme', 'algo'));
assertType('lowercase-string&non-falsy-string', wp_hash('data', 'scheme', 'algo'));
assertType('lowercase-string&non-falsy-string', wp_hash(Faker::string(), 'scheme', 'algo'));

assertType('non-falsy-string', wp_fast_hash(''));
assertType('non-falsy-string', wp_fast_hash('message'));
assertType('non-falsy-string', wp_fast_hash(Faker::string()));
