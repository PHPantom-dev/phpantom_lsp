<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_html_split_regex;
use function get_shortcode_regex;
use function get_shortcode_atts_regex;
use function get_tag_regex;
use function wp_html_split;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', get_shortcode_regex());
assertType('non-falsy-string', get_shortcode_regex(Faker::array()));

assertType('non-falsy-string', get_shortcode_atts_regex());

assertType('non-falsy-string', get_tag_regex(Faker::string()));

assertType('non-falsy-string', get_html_split_regex());

assertType('non-empty-list<string>', wp_html_split(Faker::string()));
