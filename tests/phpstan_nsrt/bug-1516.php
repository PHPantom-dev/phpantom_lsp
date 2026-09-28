<?php

namespace Bug1516;

use function PHPStan\Testing\assertType;

class FlowNodeManager
{
	public function _test() : void
	{
		$out = [];
		$a =
			[
				'foof' => 'barr',
				'ftt' => []
			];

		foreach ($a as $k => $b) {
			$str = 'toto';
			assertType('\'toto\'|array{}', $out[$k]);

			if (is_array($b)) {
				// $out[$k] is redefined there before the array_merge
				assertType('\'toto\'|array{}', $out[$k]);
				$out[$k] = [];
				assertType('array{}', $out[$k]);
				$out[$k] = array_merge($out[$k], []);
				assertType('array{}', $out[$k]);

			} else {
				// I think phpstan takes this definition as a string and takes no account of the foreach
				$out[$k] = $str;
				assertType('\'toto\'', $out[$k]);
			}
		}
	}
}
