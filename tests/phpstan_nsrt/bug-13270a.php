<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug13270a;

use function PHPStan\Testing\assertType;

final class HelloWorld
{
	/**
	 * @param array<mixed> $data
	 */
	public function test(array $data): void
	{
		foreach($data as $k => $v) {
			$data[$k]['a'] = true;
			assertType("non-empty-array<mixed>", $data); // SKIP: writing below a `mixed` element replaces every element's type
			foreach($data[$k] as $val) {
			}
		}
	}

	public function doFoo(
		mixed $mixed,
		mixed $mixed2,
		mixed $mixed3,
		mixed $mixed4,
		int $i,
		int $i2,
		string|int $stringOrInt
	): void
	{
		$mixed[$i]['a'] = true;
		assertType('mixed', $mixed);

		$mixed2[$stringOrInt]['a'] = true;
		assertType('mixed', $mixed2);

		$mixed3[$i][$stringOrInt] = true;
		assertType('mixed', $mixed3);

		$mixed4['a'][$stringOrInt] = true;
		assertType('mixed', $mixed4);

		$null = null;
		$null[$i]['a'] = true;
		assertType('non-empty-array<int, array{a: true}>', $null);

		$i2['a'] = true;
	}

	public function mixedIntoForeach(mixed $m): void
	{
		foreach ($m as $k => $v) {
		}
	}
}
