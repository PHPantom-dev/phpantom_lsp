<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_media_item;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', get_media_item(1));
assertType('non-falsy-string', get_media_item(1, null));
assertType('non-falsy-string', get_media_item(1, ''));
assertType('non-falsy-string', get_media_item(1, ['show_title' => true]));
assertType('non-falsy-string', get_media_item(1, 'arg=value'));
assertType('non-falsy-string', get_media_item(Faker::int()));
assertType('non-falsy-string', get_media_item(Faker::int(), Faker::string()));
assertType('non-falsy-string', get_media_item(Faker::int(), Faker::array()));
