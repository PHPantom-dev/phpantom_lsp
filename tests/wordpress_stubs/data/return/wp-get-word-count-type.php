<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_word_count_type;
use function PHPStan\Testing\assertType;

assertType("'characters_excluding_spaces'|'characters_including_spaces'|'words'", wp_get_word_count_type());
