<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_media_insert_url_form;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', wp_media_insert_url_form());
assertType('non-falsy-string', wp_media_insert_url_form(''));
assertType('non-falsy-string', wp_media_insert_url_form('image'));
assertType('non-falsy-string', wp_media_insert_url_form('not-image'));
assertType('non-falsy-string', wp_media_insert_url_form(Faker::string()));
