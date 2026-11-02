<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function paginate_links;
use function PHPStan\Testing\assertType;

// Returns null
assertType('null', paginate_links(['total' => Faker::negativeInt(), 'key' => 'value']));
assertType('null', paginate_links(['total' => 0, 'key' => 'value']));
assertType('null', paginate_links(['total' => 1, 'key' => 'value']));

// Returns list
assertType('list<string>', paginate_links(['type' => 'array', 'key' => 'value']));

// Returns string
assertType('string', paginate_links(['type' => 'plain', 'key' => 'value']));
assertType('string', paginate_links(['type' => 'list', 'key' => 'value']));
assertType('string', paginate_links(['type' => 'thing', 'key' => 'value']));

// Returns string by default
assertType('string', paginate_links(['key' => 'value']));
