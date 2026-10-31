<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_extract_urls;
use function PHPStan\Testing\assertType;

assertType('array{}', wp_extract_urls('')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged

assertType('list<string>', wp_extract_urls('content with no URLs'));
assertType('list<string>', wp_extract_urls('content with one URL: https://example.com'));
assertType('list<string>', wp_extract_urls('content with multiple URLs: https://example.com and http://another-example.com'));

assertType('list<string>', wp_extract_urls(Faker::string())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
