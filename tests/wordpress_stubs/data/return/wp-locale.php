<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

assertType("'characters_excluding_spaces'|'characters_including_spaces'|'words'", Faker::wpLocale()->word_count_type);
assertType("'characters_excluding_spaces'|'characters_including_spaces'|'words'", Faker::wpLocale()->get_word_count_type());
