<?php

namespace Bug12927;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param list<array{abc: string}> $list
	 * @return list<array<string>>
	 */
	public function sayHello(array $list): array
	{
		foreach($list as $k => $v) {
			unset($list[$k]['abc']);
			assertType('array{}|array{abc: string}', $list[$k]);
		}
		return $list;
	}

	/**
	 * @param list<array<string, string>> $list
	 */
	public function sayFoo(array $list): void
	{
		foreach($list as $k => $v) {
			unset($list[$k]['abc']);
			assertType('array<string, string>', $list[$k]);
		}
		assertType('list<array<string, string>>', $list); // SKIP: writes through a list's own keys drop list (and mark it non-empty), while unset() of an element keeps it
	}

	/**
	 * @param list<array<string, string>> $list
	 */
	public function sayFoo2(array $list): void
	{
		foreach($list as $k => $v) {
			$list[$k]['abc'] = 'world';
		}
	}

	/**
	 * @param list<array<string, string>> $list
	 */
	public function sayFooBar(array $list): void
	{
		foreach($list as $k => $v) {
			if (rand(0,1)) {
				unset($list[$k]);
			}
			assertType('array<int, array<string, string>>', $list); // SKIP: writes through a list's own keys drop list (and mark it non-empty), while unset() of an element keeps it
			assertType('array<string, string>', $list[$k]);
		}
		assertType('array<string, string>', $list[$k]);
	}
}
