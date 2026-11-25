<?php

namespace Bug2648;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param bool[] $list
	 */
	public function doFoo(array $list): void
	{
		if (count($list) > 1) {
			unset($list['fooo']);
			assertType('int<0, max>', count($list));
		}
	}

	/**
	 * @param bool[] $list
	 */
	public function doBar(array $list): void
	{
		if (count($list) > 1) {
			foreach ($list as $key => $item) {
				if ($item === false) {
					unset($list[$key]);
					assertType('int<0, max>', count($list));
				}

				assertType('int<0, max>', count($list));

				if (count($list) === 1) {
					assertType('1', count($list));
					$list[] = false;
					break;
				}
			}

			assertType('int<0, max>', count($list));
		}

		assertType('int<0, max>', count($list));
	}

}
