<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_comments_pagination_arrow;
use function get_query_pagination_arrow;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string|null', get_comments_pagination_arrow(Faker::wpBlock()));
assertType('non-falsy-string|null', get_comments_pagination_arrow(Faker::wpBlock(), 'next'));
assertType('non-falsy-string|null', get_comments_pagination_arrow(Faker::wpBlock(), 'previous'));
assertType('non-falsy-string|null', get_comments_pagination_arrow(Faker::wpBlock(), Faker::string()));

assertType('non-falsy-string|null', get_query_pagination_arrow(Faker::wpBlock(), true));
assertType('non-falsy-string|null', get_query_pagination_arrow(Faker::wpBlock(), false));
assertType('non-falsy-string|null', get_query_pagination_arrow(Faker::wpBlock(), Faker::bool()));
