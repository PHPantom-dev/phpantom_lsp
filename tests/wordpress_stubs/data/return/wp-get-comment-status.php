<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_get_comment_status;
use function PHPStan\Testing\assertType;

assertType("'approved'|'spam'|'trash'|'unapproved'|false", wp_get_comment_status(0));
assertType("'approved'|'spam'|'trash'|'unapproved'|false", wp_get_comment_status(123));
assertType("'approved'|'spam'|'trash'|'unapproved'|false", wp_get_comment_status(Faker::int()));
assertType("'approved'|'spam'|'trash'|'unapproved'|false", wp_get_comment_status(Faker::wpComment()));
