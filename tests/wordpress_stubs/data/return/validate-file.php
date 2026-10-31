<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function validate_file;
use function PHPStan\Testing\assertType;

$allowedFiles = Faker::array(Faker::string());

assertType('0|1|2', validate_file(Faker::string())); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('0|1|2', validate_file(Faker::string(), [])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('0|1|2|3', validate_file(Faker::string(), $allowedFiles));

assertType('0', validate_file(''));
assertType('0', validate_file('', []));
assertType('0', validate_file('', $allowedFiles));

assertType('0|1|2', validate_file('file2')); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('0|1|2', validate_file('file2', [])); // SKIP: a condition over an integer range, a refined string, `numeric`, `empty`, `resource` or an array key type is misjudged
assertType('0|1|2|3', validate_file('file2', $allowedFiles));
