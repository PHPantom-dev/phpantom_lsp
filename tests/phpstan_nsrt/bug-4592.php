<?php

namespace Bug4592;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @var array<string, array{name: string, email: string}>
	 */
	private $contacts1 = [];

	/**
	 * @var array{names: array<string, string>, emails: array<string, string>}
	 */
	private $contacts2 = ['names' => [], 'emails' => []];

	public function sayHello1(string $id): void
	{
		$name = $this->contacts1[$id]['name'] ?? null;
		assertType('string|null', $name);
	}

	public function sayHello2(string $id): void
	{
		$name = $this->contacts2['names'][$id] ?? null;
		assertType('string|null', $name);
	}
}
