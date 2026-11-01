<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function has_action;
use function has_filter;
use function PHPStan\Testing\assertType;

// Default callback of false
assertType('bool', has_filter(''));
assertType('bool', has_action(''));

// Explicit callback of false
assertType('bool', has_filter('', false));
assertType('bool', has_action('', false));

// Explicit callback
assertType('int|false', has_filter('', 'intval'));
assertType('int|false', has_action('', 'intval'));

// Maybe false callback
$callback = Faker::union(Faker::callable(), Faker::string(), Faker::array(), Faker::false());
assertType('bool|int', has_filter('', $callback));
assertType('bool|int', has_action('', $callback));
