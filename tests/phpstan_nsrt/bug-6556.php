<?php declare(strict_types = 1);

namespace Bug6556;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-type Test array{
 *     test1?: string,
 *     test2?: string,
 *     test3?: array{title: string, details: string}
 * }
 */
class TestClass {
	/**
	 * @param Test $testArg
	 */
	function testFunc(array $testArg): void {
		assertType('array{test1?: string, test2?: string, test3?: array{title: string, details: string}}', $testArg);
		$testKeys = [
			'test1',
			'test2'
		];

		$result = '';

		foreach ($testKeys as $option) {
			if (\array_key_exists($option, $testArg)) {
				$result .= '<p>' . $testArg[$option] . '</p>';
			}
		}

		assertType('array{test1?: string, test2?: string, test3?: array{title: string, details: string}}', $testArg);

		if (\array_key_exists('test3', $testArg)) {
			$result .= '<p>';
			$result .= '<b>' . $testArg['test3']['title'] . '</b><br>';
			$result .= $testArg['test3']['details'];
			$result .= '</p>';
		}
	}
}
