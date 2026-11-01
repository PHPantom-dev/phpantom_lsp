<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function rest_ensure_response;
use function PHPStan\Testing\assertType;

assertType('WP_Error', rest_ensure_response(Faker::wpError()));
assertType('WP_REST_Response', rest_ensure_response([]));
assertType('WP_REST_Response', rest_ensure_response(Faker::wpRestResponse()));
assertType('WP_Error|WP_REST_Response', rest_ensure_response(Faker::mixed()));
