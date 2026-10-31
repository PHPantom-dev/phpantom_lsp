<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_caption_input_textarea;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', wp_caption_input_textarea(Faker::wpPost()));
