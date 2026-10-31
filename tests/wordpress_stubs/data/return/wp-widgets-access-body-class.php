<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_widgets_access_body_class;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', wp_widgets_access_body_class(''));
assertType('non-falsy-string', wp_widgets_access_body_class('class1 class2'));
assertType('non-falsy-string', wp_widgets_access_body_class(Faker::string()));
