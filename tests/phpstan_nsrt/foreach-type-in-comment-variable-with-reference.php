<?php

namespace TypeInCommentOnForeach;

use function PHPStan\Testing\assertType;

/** @var mixed[] $values */
$values = [];

/** @var string $value */
foreach ($values as &$value) {
	assertType('string', $value);
}
