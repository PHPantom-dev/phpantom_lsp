<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function image_size_input_fields;
use function PHPStan\Testing\assertType;

assertType("array{label: string, input: 'html', html: string}", image_size_input_fields(Faker::wpPost()));
assertType("array{label: string, input: 'html', html: string}", image_size_input_fields(Faker::wpPost(), true));
assertType("array{label: string, input: 'html', html: string}", image_size_input_fields(Faker::wpPost(), false));
assertType("array{label: string, input: 'html', html: string}", image_size_input_fields(Faker::wpPost(), Faker::bool()));
assertType("array{label: string, input: 'html', html: string}", image_size_input_fields(Faker::wpPost(), Faker::string()));
