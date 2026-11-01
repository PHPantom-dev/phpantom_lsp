<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_current_blog_id;
use function get_current_user_id;
use function PHPStan\Testing\assertType;

assertType('int<0, max>', get_current_blog_id());

assertType('int<0, max>', get_current_user_id());
