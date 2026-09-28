<?php

namespace Bug2218;

use function PHPStan\Testing\assertType;

class Foo
{

	public function doFoo()
	{
		$fun = function (): array {
			return [
				['group_id' => 'a', 'user_id' => 'id1'],
				['group_id' => 'a', 'user_id' => 'id2'],
				['group_id' => 'a', 'user_id' => 'id3'],
				['group_id' => 'b', 'user_id' => 'id4'],
				['group_id' => 'b', 'user_id' => 'id5'],
				['group_id' => 'b', 'user_id' => 'id6'],
			];
		};

		$orders = $fun();

		$result = [];
		foreach ($orders as $order) {
			assertType('bool', isset($result[$order['group_id']]['users']));
			if (isset($result[$order['group_id']]['users'])) {
				$result[$order['group_id']]['users'][] = $order['user_id'];
				continue;
			}

			$result[$order['group_id']] = [
				'users' => [
					$order['user_id'],
				],
			];
		}
	}

}
