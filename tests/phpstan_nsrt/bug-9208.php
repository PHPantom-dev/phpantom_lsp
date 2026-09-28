<?php

namespace Bug9208;

use function PHPStan\Testing\assertType;

/**
 * @param int|non-empty-list<int> $id_or_ids
 * @return non-empty-list<int>
 */
function f(int|array $id_or_ids): array
{
	if (is_array($id_or_ids)) {
		assertType('non-empty-list<int>', (array)$id_or_ids);
	} else {
		assertType('array{int}', (array)$id_or_ids);
	}

	$ids = (array)$id_or_ids;
	assertType('non-empty-list<int>', $ids);
	return $ids;
}
