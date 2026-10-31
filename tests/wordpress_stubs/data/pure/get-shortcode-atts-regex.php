<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_shortcode_atts_regex;
use function PHPStan\Testing\assertType;

if (get_shortcode_atts_regex() === 'foo') {
    assertType("'foo'", get_shortcode_atts_regex());
}
