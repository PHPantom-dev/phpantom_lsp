<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_compat_media_markup;
use function PHPStan\Testing\assertType;

// Default output
assertType('array{item: string, meta: string}', get_compat_media_markup(Faker::nonNegativeInt()));
assertType('array{item: string, meta: string}', get_compat_media_markup(Faker::nonNegativeInt(), []));
assertType('array{item: string, meta: string}', get_compat_media_markup(Faker::nonNegativeInt(), ['key' => 'value']));
assertType('array{item: string, meta: string}', get_compat_media_markup(Faker::nonNegativeInt(), Faker::array()));
