<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function check_ajax_referer;
use function PHPStan\Testing\assertType;

$action = Faker::union(Faker::string(), Faker::int());
$queryArg = Faker::union(Faker::string(), Faker::bool());

assertType('1|2', check_ajax_referer($action, $queryArg));
assertType('1|2', check_ajax_referer($action, $queryArg, true));
assertType('1|2|false', check_ajax_referer($action, $queryArg, false));
assertType('1|2|false', check_ajax_referer($action, $queryArg, Faker::bool()));
