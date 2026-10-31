<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_widget_rss_process;
use function PHPStan\Testing\assertType;

assertType('array{title: string, url: string, link: string, items: int<1, 20>, error: string|false, show_summary: int, show_author: int, show_date: int}', wp_widget_rss_process(Faker::array()));
