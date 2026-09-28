<?php declare(strict_types = 1);

namespace Bug14178;

use function PHPStan\Testing\assertType;

class HelloWorld {
	/**
	 * @return list<string>
	 */
	public static function diff(
		?self $previousVersion,
		?self $newVersion,
	): array {
		$previousVersionExists = $previousVersion !== null;
		$newVersionExists = $newVersion !== null;

		if (!$previousVersionExists && !$newVersionExists) {
			return [];
		}

		if ($previousVersionExists && !$newVersionExists) {
			return ['bar'];
		}

		if (!$previousVersionExists) {
			assertType('true', $newVersionExists);
			assertType('Bug14178\\HelloWorld', $newVersion);
			$result = [];
			$result[] = 'foo';
			$categoryString = implode(', ', $newVersion->getSomething());
		}

		return [];
	}

	/**
	 * @return array<string>
	 */
	private function getSomething(): array {
		return ['foo'];
	}
}
