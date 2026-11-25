<?php

namespace Bug9394;

use function PHPStan\Testing\assertType;

class Order
{
	public bool $is_pre_order;
}

function (?Order $order): void {
	if ($order?->is_pre_order === false) {
		return;
	}

	assertType(Order::class . '|null', $order);
};
