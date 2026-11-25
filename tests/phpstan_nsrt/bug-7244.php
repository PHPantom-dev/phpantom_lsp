<?php declare(strict_types = 1);

namespace Bug7244;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @param array<string, mixed> $arguments
	 */
	public function getFormat(array $arguments): string {
		$value = \is_string($arguments['format'] ?? null) ? $arguments['format'] : 'Y-m-d';
		assertType('string', $value);
		return $value;
	}

	/**
	 * @param array<string, mixed> $arguments
	 */
	public function getFormatWithoutFallback(array $arguments): string {
		$value = \is_string($arguments['format']) ? $arguments['format'] : 'Y-m-d';
		assertType('string', $value);
		return $value;
	}
}
