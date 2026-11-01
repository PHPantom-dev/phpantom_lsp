<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

$translations = Faker::wpTranslations();

$string = Faker::string();
$singular = Faker::union(Faker::string(), null);

// WP_Translation::translate()
assertType('null', $translations->translate(null));
assertType('string', $translations->translate($string));

// WP_Translation::translate_plural()
assertType('null', $translations->translate_plural(null, null));
assertType('string', $translations->translate_plural($string, null));
assertType('null', $translations->translate_plural(null, $string));
assertType('string', $translations->translate_plural($string, $string));
assertType('string|null', $translations->translate_plural($singular, $string));
