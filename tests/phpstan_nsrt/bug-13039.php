<?php declare(strict_types=1);

namespace Bug13039;

use function PHPStan\Testing\assertType;

function doFoo() {
	/** @var list<array<string, mixed>> */
	$transactions = [];

	assertType('list<array<string, mixed>>', $transactions);

	foreach (array_keys($transactions) as $k) {
		$transactions[$k]['Shares'] = [];
		$transactions[$k]['Shares']['Projects'] = [];
		$transactions[$k]['Shares']['People'] = [];
	}

}

