<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function image_link_input_fields;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', image_link_input_fields(Faker::wpPost()));
assertType('non-falsy-string', image_link_input_fields(Faker::wpPost(), ''));
assertType('non-falsy-string', image_link_input_fields(Faker::wpPost(), 'url_type'));
assertType('non-falsy-string', image_link_input_fields(Faker::wpPost(), Faker::string()));
