<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use stdClass;

use function maybe_serialize;
use function PHPStan\Testing\assertType;

assertType('null', maybe_serialize(null));
assertType('true', maybe_serialize(true));
assertType('false', maybe_serialize(false));
assertType('bool', maybe_serialize(Faker::bool()));
assertType('1', maybe_serialize(1));
assertType('int', maybe_serialize(Faker::int()));
assertType('0.0', maybe_serialize(0.0)); // SKIP: a float literal is typed `float`, not its value
assertType('float', maybe_serialize(Faker::float()));
assertType('string', maybe_serialize(''));
assertType('string', maybe_serialize('foo'));
assertType('string', maybe_serialize(Faker::string()));
assertType('string', maybe_serialize([]));
assertType('string', maybe_serialize(['foo' => 'bar']));
assertType('string', maybe_serialize(Faker::array()));
assertType('string', maybe_serialize(new stdClass()));
