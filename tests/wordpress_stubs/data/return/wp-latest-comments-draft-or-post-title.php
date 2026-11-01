<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_latest_comments_draft_or_post_title;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', wp_latest_comments_draft_or_post_title(Faker::int()));
assertType('non-falsy-string', wp_latest_comments_draft_or_post_title(Faker::wpPost()));
