<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_is_uuid;
use function PHPStan\Testing\assertType;

$uuid = Faker::string();

if (wp_is_uuid($uuid)) {
    assertType('lowercase-string&non-falsy-string', $uuid); // SKIP: `@phpstan-assert` narrowing intersects refined string types wrongly
}
assertType('string', $uuid); // SKIP: `@phpstan-assert` narrowing intersects refined string types wrongly

if (! wp_is_uuid($uuid)) {
    assertType('string', $uuid);
    return;
}
assertType('lowercase-string&non-falsy-string', $uuid); // SKIP: `@phpstan-assert` narrowing intersects refined string types wrongly

$uuid = 'foo';

if (wp_is_uuid($uuid)) {
    assertType("'foo'", $uuid); // SKIP: `@phpstan-assert` narrowing intersects refined string types wrongly
}
assertType("'foo'", $uuid);

if (! wp_is_uuid($uuid)) {
    assertType("'foo'", $uuid);
    return;
}
assertType("'foo'", $uuid); // SKIP: `@phpstan-assert` narrowing intersects refined string types wrongly
