<?php

namespace TypeInCommentOnForeach;

use function PHPStan\Testing\assertType;

/** @var mixed[] $values */
$values = [];

/** @var \stdClass $value */
foreach ($values as $value) {
	assertType('stdClass', $value);
}
