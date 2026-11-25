<?php declare(strict_types = 1);

namespace Bug12447;

use function PHPStan\Testing\assertType;

assertType('mixed', $data);
$data[] = 1;
assertType('array{1}', $data); // PHPStan says `mixed`, allowing for a file that includes this one to have set `$data`

function (): void {
	assertType('*ERROR*', $data);
	$data[] = 1;
	assertType('array{1}', $data);
};
