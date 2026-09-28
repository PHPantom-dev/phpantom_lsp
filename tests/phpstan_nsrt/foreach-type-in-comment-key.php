<?php

namespace TypeInCommentOnForeach;

use function PHPStan\Testing\assertType;

/** @var mixed[] $values */
$values = [];

/** @var int $key */
foreach ($values as $key => $value) {
	assertType('int', $key);
}
