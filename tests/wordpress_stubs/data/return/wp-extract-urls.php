<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_extract_urls;
use function PHPStan\Testing\assertType;

assertType('array{}', wp_extract_urls(''));

assertType('list<string>', wp_extract_urls('content with no URLs'));
assertType('list<string>', wp_extract_urls('content with one URL: https://example.com'));
assertType('list<string>', wp_extract_urls('content with multiple URLs: https://example.com and http://another-example.com'));

assertType('array{}|list<string>', wp_extract_urls(Faker::string())); // PHPantom keeps the array types apart where PHPStan merges them
