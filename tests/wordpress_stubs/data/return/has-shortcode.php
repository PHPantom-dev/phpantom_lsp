<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

assertType('false', has_shortcode('', ''));
assertType('false', has_shortcode('', 'foo'));
assertType('false', has_shortcode('foo', ''));
assertType('bool', has_shortcode('foo', 'foo'));

assertType('false', has_shortcode('', ''));
assertType('false', has_shortcode('', Faker::string()));
assertType('false', has_shortcode(Faker::string(), ''));
assertType('bool', has_shortcode(Faker::string(), Faker::string()));
