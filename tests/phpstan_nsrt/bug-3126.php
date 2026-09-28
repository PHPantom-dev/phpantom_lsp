<?php

namespace Bug3126;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<int,string> $input
	 */
	function failure(array $input): void {
		$results = [];

		foreach ($input as $keyOne => $layerOne) {
			assertType('bool', isset($results[$keyOne]['name']));
			if(isset($results[$keyOne]['name']) === false) {
				$results[$keyOne]['name'] = $layerOne;
			}
		}
	}

	/**
	 * @param array<int,string> $input
	 */
	function no_failure(array $input): void {
		$results = [];

		foreach ($input as $keyOne => $layerOne) {
			if(isset($results[$keyOne]) === false) {
				$results[$keyOne] = $layerOne;
			}
		}
	}

}
