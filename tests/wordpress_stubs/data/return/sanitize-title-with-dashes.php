<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function sanitize_title_with_dashes;
use function PHPStan\Testing\assertType;

assertType('lowercase-string', sanitize_title_with_dashes(''));
assertType('lowercase-string', sanitize_title_with_dashes('title'));
assertType('lowercase-string', sanitize_title_with_dashes(Faker::string()));

assertType('lowercase-string', sanitize_title_with_dashes('', '', 'display'));
assertType('lowercase-string', sanitize_title_with_dashes('title', '', 'display'));
assertType('lowercase-string', sanitize_title_with_dashes(Faker::string(), '', 'display'));

assertType('lowercase-string', sanitize_title_with_dashes('', '', 'save'));
assertType('lowercase-string', sanitize_title_with_dashes('title', '', 'save'));
assertType('lowercase-string', sanitize_title_with_dashes(Faker::string(), '', 'save'));

assertType('lowercase-string', sanitize_title_with_dashes('', '', Faker::string()));
assertType('lowercase-string', sanitize_title_with_dashes('title', '', Faker::string()));
assertType('lowercase-string', sanitize_title_with_dashes(Faker::string(), '', Faker::string()));

assertType('lowercase-string', sanitize_title_with_dashes('', Faker::string(), Faker::string()));
assertType('lowercase-string', sanitize_title_with_dashes('title', Faker::string(), Faker::string()));
assertType('lowercase-string', sanitize_title_with_dashes(Faker::string(), Faker::string(), Faker::string()));
