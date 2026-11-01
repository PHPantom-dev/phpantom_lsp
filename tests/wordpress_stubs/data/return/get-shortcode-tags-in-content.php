<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_shortcode_tags_in_content;
use function PHPStan\Testing\assertType;

assertType('list<non-empty-string>', get_shortcode_tags_in_content(''));
assertType('list<non-empty-string>', get_shortcode_tags_in_content('0'));
assertType('list<non-empty-string>', get_shortcode_tags_in_content('foo'));
assertType('list<non-empty-string>', get_shortcode_tags_in_content('foo [bar] baz'));
assertType('list<non-empty-string>', get_shortcode_tags_in_content(Faker::nonEmptyString()));
assertType('list<non-empty-string>', get_shortcode_tags_in_content(Faker::nonFalsyString()));
assertType('list<non-empty-string>', get_shortcode_tags_in_content(Faker::string()));
