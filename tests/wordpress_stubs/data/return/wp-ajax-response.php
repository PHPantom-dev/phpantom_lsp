<?php

declare(strict_types=1);

use PhpStubs\WordPress\Core\Tests\Faker;

use function PHPStan\Testing\assertType;

$wpAjaxResponse = new WP_Ajax_Response([]);

assertType('non-falsy-string', $wpAjaxResponse->add(Faker::array()));
assertType('non-falsy-string', $wpAjaxResponse->add(Faker::string()));
