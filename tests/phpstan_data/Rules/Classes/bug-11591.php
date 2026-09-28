<?php

namespace Bug11591;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-type SettingsFactory callable(static): array<string,mixed>
 */
trait WithConfig {
	/**
	 * @param SettingsFactory $settings
	 */
	public function setConfig(callable $settings): void {
		$settings($this);
	}

	/**
	 * @param callable(static): array<string,mixed> $settings
	 */
	public function setConfig2(callable $settings): void {
		$settings($this);
	}

	/**
	 * @param callable(self): array<string,mixed> $settings
	 */
	public function setConfig3(callable $settings): void {
		$settings($this);
	}
}

class A
{
	use WithConfig;
}

function (A $a): void {
	$a->setConfig(function ($who) {
		assertType(A::class, $who);

		return [];
	});
};
