<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_available_post_statuses;
use function PHPStan\Testing\assertType;

assertType('list<string>', get_available_post_statuses());
assertType('list<string>', get_available_post_statuses(''));
assertType('list<string>', get_available_post_statuses('post'));
assertType('list<string>', get_available_post_statuses('postType'));
assertType('list<string>', get_available_post_statuses(Faker::string()));
