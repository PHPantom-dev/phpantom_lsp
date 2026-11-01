<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_html_split_regex;
use function PHPStan\Testing\assertType;

if (get_html_split_regex() === 'foo') {
    assertType("'foo'", get_html_split_regex());
}
