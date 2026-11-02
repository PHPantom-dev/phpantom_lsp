<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function do_meta_boxes;
use function PHPStan\Testing\assertType;

assertType('int<0, max>', do_meta_boxes(Faker::string(), Faker::string(), Faker::mixed()));
