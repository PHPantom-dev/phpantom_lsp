<?php

namespace Bug4209;

use function PHPStan\Testing\assertType;

/**
 * @template T
 */
class Link {
	/**
	 * @var T
	 */
	public $item;

	/**
	 * @param T $item
	 */
	public function __construct($item) {
		$this->item = $item;
	}
}

class Customer
{
	public function getName(): string { return 'customer'; }
}

/**
 * @return Link<Customer>[]
 */
function get_links(): array {
	return [new Link(new Customer())];
}

/**
 * @template T
 * @param Link<T>[] $links
 * @return T
 */
function process_customers(array $links) {
	// no-op
}

class Runner {
	public function run(): void
	{
		assertType('Bug4209\Customer', process_customers(get_links()));
	}
}
